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
use fm_agent::{Message, ToolSpec, render_text};
use fm_credentials::{CredentialRef, CredentialStore, SecretMaterial, StoreCredentialRequest};
use fm_settings::{SettingsStore, VersionedDocument};
use fm_transport_dto::{
    LlmAdvancedSettingsDto, LlmApiCapabilityDto, LlmEndpointLocalityDto, LlmModelLimitsDto,
    LlmPresetDto, LlmProfileDto, LlmProfileExportDto, LlmProfilePresetDto, LlmProfileTestResultDto,
    LlmRoleAssignmentDto, LlmRoleDto, LlmRolesDto, LlmTestErrorCategoryDto, LlmTlsPolicyDto,
    OrphanLlmCredentialDispositionDto, SaveLlmProfileRequestDto, SaveLlmRoleRequestDto,
    ThinkingEffortDto,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};
use uuid::Uuid;

use crate::image_generation_transport::{ImageApiStyle, ImageEndpointRequest};

const PROFILE_SCHEMA_VERSION: u32 = 6;
/// The answer cap every profile had by default before schema 6.
const LEGACY_DEFAULT_ANSWER_TOKENS: u64 = 1_024;
const TEST_PROMPT: &str = "Reply with OK.";
/// Most bytes of one raw response body (server-sent events carry far more JSON than text). Only a
/// runaway generation reaches it; it replaces an answer-token cap as the guard against one.
const MAX_STREAM_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Most bytes of one reply's text, and of one native call's arguments (about 250k tokens).
const MAX_REPLY_BYTES: usize = 1024 * 1024;
/// Hard application bound on the context window, independent of editable profile or provider
/// metadata. An answer limit, when the person sets one, is bounded by the context window only.
pub const MAX_GENERATION_CONTEXT_WINDOW: u32 = 1_048_576;
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

/// How much a model should think before it answers (task 0361). The chat role stores a default;
/// a conversation may choose its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingEffort {
    /// As before there was a choice: thinking off on local servers, the provider's own default in
    /// the cloud.
    #[default]
    Auto,
    Off,
    Low,
    Medium,
    High,
}

impl ThinkingEffort {
    /// The `reasoning_effort` / Ollama `think` level of a thinking effort.
    const fn level(self) -> Option<&'static str> {
        match self {
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::Auto | Self::Off => None,
        }
    }
}

/// Which thinking effort a generation asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThinkingRequest {
    /// A background call (a summary, a name, a JSON task): [`ThinkingEffort::Auto`], so it stays
    /// fast.
    #[default]
    Background,
    /// A reply to the person: the conversation's choice, or the role's default without one.
    Conversation(Option<ThinkingEffort>),
}

impl ThinkingRequest {
    /// The effort to use when the role's default is `default`.
    pub fn effort(self, default: ThinkingEffort) -> ThinkingEffort {
        match self {
            Self::Background => ThinkingEffort::Auto,
            Self::Conversation(choice) => choice.unwrap_or(default),
        }
    }
}

/// What Ollama's `think` field carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OllamaThink {
    Off,
    On,
    Level(&'static str),
}

impl OllamaThink {
    fn value(self) -> Value {
        match self {
            Self::Off => Value::Bool(false),
            Self::On => Value::Bool(true),
            Self::Level(level) => Value::from(level),
        }
    }
}

/// One way of asking an endpoint for a thinking effort; [`thinking_attempts`] lists them in the
/// order they are tried until the endpoint accepts one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingAttempt {
    pub switch: ReasoningSwitch,
    /// `reasoning_effort` for an OpenAI-compatible endpoint.
    pub effort: Option<&'static str>,
    /// `think` for Ollama.
    pub ollama_think: OllamaThink,
    /// A chat-template switch (`chat_template_kwargs`) and its value, for a model whose template
    /// is known ([`crate::model_thinking`]).
    pub template: Option<(&'static str, bool)>,
}

impl ThinkingAttempt {
    const PLAIN: Self = Self {
        switch: ReasoningSwitch::Plain,
        effort: None,
        ollama_think: OllamaThink::Off,
        template: None,
    };

    const fn switch(switch: ReasoningSwitch) -> Self {
        Self {
            switch,
            ..Self::PLAIN
        }
    }

    const fn effort(effort: &'static str) -> Self {
        Self {
            effort: Some(effort),
            ..Self::PLAIN
        }
    }

    const fn ollama(think: OllamaThink) -> Self {
        Self {
            ollama_think: think,
            ..Self::PLAIN
        }
    }

    /// Whether the request carries a thinking parameter an endpoint might reject.
    fn asks_anything(self) -> bool {
        self != Self::PLAIN
    }
}

/// The ways to ask `profile`'s endpoint for `effort`, in the order they are tried; the last one
/// asks for nothing the endpoint could reject. A model whose family is listed in
/// [`crate::model_thinking`] is asked the way its chat template expects; any other model is asked
/// by trial, from the closest match down to nothing at all, since OpenAI-compatible servers do not
/// disclose which values a model takes.
pub fn thinking_attempts(profile: &LlmProfile, effort: ThinkingEffort) -> Vec<ThinkingAttempt> {
    thinking_attempts_with(
        profile,
        crate::model_thinking::lookup(&profile.model),
        effort,
    )
}

/// Like [`thinking_attempts`], with what is known about the model given: learned from its
/// template (task 0362), listed, or nothing.
pub fn thinking_attempts_with(
    profile: &LlmProfile,
    known: Option<&crate::model_thinking::ModelThinking>,
    effort: ThinkingEffort,
) -> Vec<ThinkingAttempt> {
    if profile.preset == LlmPreset::Ollama {
        return ollama_attempts(profile, known, effort);
    }
    if let Some(entry) = known {
        let local = supports_reasoning_switch(profile.preset);
        let mut attempts = Vec::with_capacity(3);
        let first = known_attempt(entry, effort, local);
        // A family fallback shares the switch, not verified levels: try the plain level name
        // first, and keep the switch alone if the server refuses it.
        if entry.family
            && first.effort.is_none()
            && let Some(level) = effort.level()
        {
            attempts.push(ThinkingAttempt {
                effort: Some(level),
                ..first
            });
        }
        attempts.push(first);
        if first != ThinkingAttempt::PLAIN {
            attempts.push(ThinkingAttempt::PLAIN);
        }
        return attempts;
    }
    if let Some(level) = effort.level() {
        return vec![ThinkingAttempt::effort(level), ThinkingAttempt::PLAIN];
    }
    if supports_reasoning_switch(profile.preset) {
        // Off and Auto alike: the thinking-off switch this server took last time goes first.
        let mut attempts: Vec<ThinkingAttempt> = profile
            .reasoning_switch
            .into_iter()
            .map(ThinkingAttempt::switch)
            .collect();
        attempts.extend(
            ReasoningSwitch::LADDER
                .into_iter()
                .filter(|switch| Some(*switch) != profile.reasoning_switch)
                .map(ThinkingAttempt::switch),
        );
        return attempts;
    }
    match effort {
        // Newer OpenAI models take `none`, the first GPT-5 models `minimal`, o-series `low` at
        // the least; a model that does not reason refuses the parameter altogether.
        ThinkingEffort::Off => vec![
            ThinkingAttempt::effort("none"),
            ThinkingAttempt::effort("minimal"),
            ThinkingAttempt::effort("low"),
            ThinkingAttempt::PLAIN,
        ],
        _ => vec![ThinkingAttempt::PLAIN],
    }
}

/// Ollama: a boolean, or a level for gpt-oss (which ignores booleans). A model without the thinking
/// capability refuses `think: true`, so a listed model that never thinks is not asked to.
fn ollama_attempts(
    profile: &LlmProfile,
    known: Option<&crate::model_thinking::ModelThinking>,
    effort: ThinkingEffort,
) -> Vec<ThinkingAttempt> {
    use crate::model_thinking::ThinkingKind;
    let off = ThinkingAttempt::ollama(OllamaThink::Off);
    let Some(level) = effort.level() else {
        return vec![off];
    };
    if known.is_some_and(|entry| entry.thinking == ThinkingKind::None) {
        return vec![off];
    }
    if profile.model.to_ascii_lowercase().contains("gpt-oss") {
        return vec![ThinkingAttempt::ollama(OllamaThink::Level(level)), off];
    }
    vec![ThinkingAttempt::ollama(OllamaThink::On), off]
}

/// The request a listed model's chat template expects for `effort`. `local` endpoints keep
/// thinking off for [`ThinkingEffort::Auto`], as they always did; elsewhere Auto asks nothing.
fn known_attempt(
    entry: &crate::model_thinking::ModelThinking,
    effort: ThinkingEffort,
    local: bool,
) -> ThinkingAttempt {
    use crate::model_thinking::{LevelName, ThinkingKind};
    let level = match effort {
        ThinkingEffort::Auto if local => LevelName::Off,
        ThinkingEffort::Auto => return ThinkingAttempt::PLAIN,
        ThinkingEffort::Off => LevelName::Off,
        ThinkingEffort::Low => LevelName::Low,
        ThinkingEffort::Medium => LevelName::Medium,
        ThinkingEffort::High => LevelName::High,
    };
    // Only switches and values a request may carry (checked when the list is loaded, too).
    let effort = entry
        .level(level)
        .and_then(crate::model_thinking::known_effort);
    match entry.thinking {
        ThinkingKind::None => ThinkingAttempt::PLAIN,
        ThinkingKind::Always => ThinkingAttempt {
            effort,
            ..ThinkingAttempt::PLAIN
        },
        ThinkingKind::Toggle => ThinkingAttempt {
            effort,
            template: entry
                .switch
                .as_deref()
                .and_then(crate::model_thinking::known_switch)
                .map(|name| (name, level != LevelName::Off)),
            ..ThinkingAttempt::PLAIN
        },
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
    /// A cap on each answer's tokens, only when the person sets one (for cost, say). `None`, the
    /// default, sends no cap: a capped answer is cut off and fails, which costs more than it saves.
    #[serde(default)]
    pub maximum_answer_tokens: Option<u32>,
    pub temperature: f32,
    pub timeout_seconds: u64,
    pub tls_policy: LlmTlsPolicy,
    pub custom_headers: BTreeMap<String, String>,
}

impl Default for LlmAdvancedSettings {
    fn default() -> Self {
        Self {
            context_window: 8_192,
            maximum_answer_tokens: None,
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
    /// Whether the model takes native tool calls, found by a background probe after the endpoint or
    /// model changed (learned, not user-set). `None` until the probe has answered.
    #[serde(default)]
    pub tool_calling: Option<ToolCallingProbe>,
}

/// What the tool-calling probe found for one model of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallingProbe {
    /// The model the probe asked; a role may use another model than the profile's.
    pub model: String,
    /// Whether the model answered the probe with a tool call.
    pub supported: bool,
}

impl LlmProfile {
    /// Whether the probe found that this profile's model takes native tool calls.
    fn takes_native_tools(&self) -> bool {
        self.tool_calling
            .as_ref()
            .is_some_and(|probe| probe.supported && probe.model == self.model)
    }
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

    /// The server posture: the administrator's host list and nothing else - not even the machine's
    /// own loopback, which would let any browser user point this server at services on itself.
    #[must_use]
    pub fn server(allowed_cloud_hosts: impl IntoIterator<Item = String>) -> Self {
        Self::server_with_loopback(allowed_cloud_hosts, false)
    }

    /// The server posture with the loopback branch opened on purpose
    /// (`PROCYON_LLM_ALLOW_LOOPBACK`, task 0325). A single-user server that runs a model on the same
    /// machine needs this; a shared one should not have it, because every browser user of that
    /// server can then make it contact its own localhost.
    #[must_use]
    pub fn server_with_loopback(
        allowed_cloud_hosts: impl IntoIterator<Item = String>,
        allow_loopback: bool,
    ) -> Self {
        Self {
            allowed_cloud_hosts: allowed_cloud_hosts
                .into_iter()
                .map(|host| host.to_ascii_lowercase())
                .collect(),
            allow_loopback,
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
    /// `reasoning_effort` to send to an OpenAI-compatible endpoint (task 0361).
    pub reasoning_effort: Option<&'static str>,
    /// Ollama's `think`.
    pub ollama_think: OllamaThink,
    /// A chat-template switch and its value (`chat_template_kwargs`), for a known model.
    pub template_switch: Option<(&'static str, bool)>,
}

impl LlmProbeRequest {
    /// This request asking for one thinking attempt.
    fn with_thinking(&self, attempt: ThinkingAttempt) -> Self {
        Self {
            reasoning_switch: attempt.switch,
            reasoning_effort: attempt.effort,
            ollama_think: attempt.ollama_think,
            template_switch: attempt.template,
            ..self.clone()
        }
    }

    /// Whether the request carries a thinking parameter an endpoint might reject.
    fn asks_thinking(&self) -> bool {
        self.reasoning_switch != ReasoningSwitch::Plain
            || self.reasoning_effort.is_some()
            || self.ollama_think != OllamaThink::Off
            || self.template_switch.is_some()
    }
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
            .field("reasoning_switch", &self.reasoning_switch)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("ollama_think", &self.ollama_think)
            .field("template_switch", &self.template_switch)
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
    /// The model takes native tool calls (Ollama reports this per model).
    pub tools: bool,
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

/// The form a generation's reply takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyFormat {
    /// Prose or Markdown, or tool calls.
    Text,
    /// One JSON value, for callers that parse the reply.
    Json,
}

/// Bounded host-owned generation request using a saved profile.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmChatGeneration {
    /// Tools the model may call natively (task 0339). Sent only to endpoints whose model was found
    /// to take them (see [`ToolCallingProbe`]); a call comes back as a `TOOL {json}` line in the
    /// reply text (with the endpoint's call id), the same form the text protocol uses.
    pub tools: Vec<ToolSpec>,
    /// Trusted system instruction.
    pub system_prompt: String,
    /// User-role evidence payload. May be empty when `followups` starts with the person's message.
    pub user_prompt: String,
    /// The conversation after `user_prompt` (task 0341): replies with their native calls, one
    /// result per call, and further user messages and notes. Sent as structured messages to an
    /// endpoint that takes the `tools`; folded into `user_prompt` as text otherwise.
    pub followups: Vec<Message>,
    /// Bounded image evidence. Only transports with verified vision support may accept it.
    pub images: Vec<LlmImageAttachment>,
    /// A cap on generated tokens, sent only when there is one: `None` lets the model finish its
    /// answer (a runaway generation is stopped by the response size bound instead). The
    /// profile's own limit, when the person set one, still applies.
    pub maximum_tokens: Option<u32>,
    /// Explicit runtime context request, used only after summary capacity is verified.
    pub context_window: Option<u32>,
    /// Sampling temperature, further bounded by the saved profile. `None` keeps the model's own
    /// default, which the server applies when the field is absent.
    pub temperature: Option<f32>,
    /// What the reply must be. A JSON reply is enforced where the endpoint can (Ollama).
    pub reply: ReplyFormat,
    /// How much the model should think (task 0361).
    pub thinking: ThinkingRequest,
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
    /// The chat template an oMLX server on this machine uses for `request.model` (task 0362):
    /// read from the model's folder, which `/v1/models/status` names. `None` for other servers
    /// or when the template cannot be found.
    async fn chat_template(
        &self,
        _request: &LlmProbeRequest,
        _cancellation: &CancellationToken,
    ) -> Result<Option<String>, LlmProfileError> {
        Ok(None)
    }

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

    /// One Ollama generation. A request asking to think that Ollama refuses (a model without the
    /// thinking capability) is [`ChatOutcome::Rejected`], so the caller can ask without.
    async fn generate_ollama(
        &self,
        request: &LlmProbeRequest,
        url: &str,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<ChatOutcome, LlmProfileError> {
        let body = ollama_generation_body(request, generation);
        let response = send_generation(
            self.generation_request(reqwest::Method::POST, request, url)
                .json(&body),
            request,
            cancellation,
        )
        .await?;
        if request.asks_thinking() && matches!(response.status().as_u16(), 400 | 422 | 500) {
            return Ok(ChatOutcome::Rejected);
        }
        let response = refused_generation(response).await?;
        let bytes = read_bounded_body(response, cancellation).await?;
        Ok(ChatOutcome::Generated {
            text: finish_reply(parse_ollama_reply(&bytes)?, generation.reply)?,
            reasoning_seen: false,
        })
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

    async fn chat_template(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<Option<String>, LlmProfileError> {
        let Some(url) = models_url(&request.url).map(|models| format!("{models}/status")) else {
            return Ok(None);
        };
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = self.request(reqwest::Method::GET, request, &url).send() => {
                response.map_err(map_reqwest)?
            }
        };
        if !response.status().is_success() {
            return Ok(None);
        }
        let body = read_bounded_body(response, cancellation).await?;
        let Some(folder) = model_folder(&body, &request.model) else {
            return Ok(None);
        };
        Ok(read_chat_template(&folder).await)
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
            return match self
                .generate_ollama(request, url, generation, cancellation)
                .await?
            {
                ChatOutcome::Generated { text, .. } => Ok(text),
                ChatOutcome::Rejected => Err(LlmProfileError::Transport),
            };
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
        if let Some(url) = &request.ollama_generation_url {
            let outcome = self
                .generate_ollama(request, url, generation, cancellation)
                .await?;
            if let ChatOutcome::Generated { text, .. } = &outcome {
                on_delta(text);
            }
            return Ok(outcome);
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
        if request.asks_thinking() && matches!(status, 400 | 422 | 500) {
            return Ok(ChatOutcome::Rejected);
        }
        let response = refused_generation(response).await?;
        let bytes = read_streamed_body(response, cancellation, request.timeout, on_delta).await?;
        let reply = parse_streaming_chat_detailed(&bytes)?;
        let reasoning_seen = reply.reasoning_seen;
        Ok(ChatOutcome::Generated {
            text: finish_reply(reply, generation.reply)?,
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
        // A chunk must arrive within the idle time; a stream that keeps arriving may run on. Until
        // the first chunk the first-reply wait applies: a server may send its headers at once and
        // then read a long prompt for minutes before the first token.
        let wait = if bytes.is_empty() {
            idle.max(FIRST_REPLY_MIN_WAIT)
        } else {
            idle
        };
        let next = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            next = tokio::time::timeout(wait, stream.next()) => {
                next.map_err(|_| LlmProfileError::Timeout)?
            }
        };
        let Some(chunk) = next else {
            // A final line without its newline still counts.
            events.push(b"\n");
            return Ok(bytes);
        };
        let chunk = chunk.map_err(map_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_STREAM_BODY_BYTES {
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
    /// The thinking effort of a reply when the conversation chose none (task 0361); used by the
    /// chat role.
    #[serde(default)]
    pub thinking: ThinkingEffort,
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
    /// The default thinking effort; `None` keeps the role's current one.
    pub thinking: Option<ThinkingEffort>,
}

/// A role binding that passed endpoint, consent and cloud-permission checks.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResolvedRole {
    pub profile: LlmProfile,
    pub model: String,
    /// The role's default thinking effort.
    pub thinking: ThinkingEffort,
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
            3..=5 => {}
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
        if version <= 5 {
            // 1024 was the default answer cap, not a choice: it becomes no cap. Any other value
            // was set on purpose and stays.
            if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
                for profile in profiles {
                    let advanced = &mut profile["advanced"];
                    if advanced.get("maximumAnswerTokens").and_then(Value::as_u64)
                        == Some(LEGACY_DEFAULT_ANSWER_TOKENS)
                    {
                        advanced["maximumAnswerTokens"] = Value::Null;
                    }
                }
            }
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
/// A profile, the model it serves and a thinking effort.
type ThinkingKey = (Uuid, String, ThinkingEffort);

#[derive(Clone)]
pub struct LlmProfileService {
    settings: SettingsStore,
    profiles: Arc<Mutex<LlmProfilesDocument>>,
    /// Tool-calling probes in flight or recently unanswered, with the time the next may start.
    probing: Arc<Mutex<std::collections::HashMap<(Uuid, String), Instant>>>,
    /// The thinking attempt each profile, model and effort accepted in this session (task 0361).
    accepted_thinking: Arc<Mutex<std::collections::HashMap<ThinkingKey, ThinkingAttempt>>>,
    /// Thinking controls learned from the templates of servers on this machine (task 0362).
    learned_thinking: Arc<crate::model_thinking::learned::LearnedThinking>,
    /// Endpoints and models whose template was looked at in this session.
    templates_checked: Arc<Mutex<std::collections::HashSet<(String, String)>>>,
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
        let learned_thinking = Arc::new(crate::model_thinking::learned::LearnedThinking::new(
            SettingsStore::new(settings.directory()),
        ));
        Ok(Self {
            settings,
            profiles: Arc::new(Mutex::new(profiles)),
            probing: Arc::default(),
            accepted_thinking: Arc::default(),
            learned_thinking,
            templates_checked: Arc::default(),
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
        let learned_thinking = Arc::new(crate::model_thinking::learned::LearnedThinking::new(
            SettingsStore::new(settings.directory()),
        ));
        Self {
            settings,
            profiles: Arc::new(Mutex::new(LlmProfilesDocument::default())),
            probing: Arc::default(),
            accepted_thinking: Arc::default(),
            learned_thinking,
            templates_checked: Arc::default(),
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

    /// The role bindings for the API, the chat role with what its model supports for thinking:
    /// learned from its template on this machine when it was read (task 0362), else listed.
    pub(crate) fn roles_dto(&self) -> Result<LlmRolesDto, LlmProfileError> {
        let (roles, base_urls) = {
            let document = self.lock()?;
            let base_urls: std::collections::HashMap<Uuid, String> = document
                .profiles
                .iter()
                .filter_map(|profile| {
                    normalize_endpoint(&profile.base_url)
                        .ok()
                        .map(|endpoint| (profile.id, endpoint.base_url))
                })
                .collect();
            (document.roles.clone(), base_urls)
        };
        let dto = |assignment: RoleAssignment| {
            let learned = base_urls
                .get(&assignment.profile_id)
                .and_then(|base| self.learned_thinking.get(base, &assignment.model));
            let support = match &learned {
                Some(learned) => Some(thinking_support_dto(&learned.thinking, Some(learned))),
                None => crate::model_thinking::lookup(&assignment.model)
                    .map(|entry| thinking_support_dto(entry, None)),
            };
            assignment_to_dto(assignment, support)
        };
        Ok(LlmRolesDto {
            chat: roles.chat.map(dto),
            embeddings: roles.embeddings.map(dto),
            image: roles.image.map(dto),
        })
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
        self.schedule_tool_probe(profile.id, profile.model.clone());
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
            profile.tool_calling = old.tool_calling.clone();
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
        if profile.tool_calling.is_none() {
            self.schedule_tool_probe(profile.id, profile.model.clone());
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
        drop(document);
        self.schedule_tool_probe(profile.id, profile.model.clone());
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
        drop(document);
        self.schedule_tool_probe(saved.id, saved.model.clone());
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
                let current = match role {
                    LlmRole::Chat => document.roles.chat.as_ref(),
                    LlmRole::Embeddings => document.roles.embeddings.as_ref(),
                    LlmRole::Image => document.roles.image.as_ref(),
                };
                Some(RoleAssignment {
                    profile_id: input.profile_id,
                    model: input.model,
                    allow_cloud: input.allow_cloud,
                    thinking: input
                        .thinking
                        .or_else(|| current.map(|current| current.thinking))
                        .unwrap_or_default(),
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
            thinking: assignment.thinking,
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
        let effort = generation.thinking.effort(ThinkingEffort::Auto);
        self.generate_profile_streamed(profile, generation, effort, cancellation, on_delta)
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
        let effort = generation.thinking.effort(resolved.thinking);
        self.generate_profile_streamed(profile, generation, effort, cancellation, on_delta)
            .await
    }

    async fn generate_profile_streamed(
        &self,
        profile: LlmProfile,
        generation: LlmChatGeneration,
        effort: ThinkingEffort,
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
        let followup_bytes: usize = generation.followups.iter().map(Message::size).sum();
        if generation.system_prompt.is_empty()
            || (generation.user_prompt.is_empty() && generation.followups.is_empty())
            || generation.system_prompt.len() > 64 * 1024
            || generation.user_prompt.len() + followup_bytes > 1024 * 1024
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
        let native = profile.takes_native_tools() && !generation.tools.is_empty();
        let (user_prompt, followups) = if native || generation.followups.is_empty() {
            (generation.user_prompt, generation.followups)
        } else {
            // An endpoint without native tools reads the tool exchanges as text after the prompt.
            let mut text = generation.user_prompt;
            if !text.is_empty() && !text.ends_with("\n\n") {
                text.push_str(if text.ends_with('\n') { "\n" } else { "\n\n" });
            }
            text.push_str(&render_text(&generation.followups));
            (text, Vec::new())
        };
        let bounded = LlmChatGeneration {
            user_prompt,
            followups,
            tools: if native {
                generation.tools.clone()
            } else {
                Vec::new()
            },
            maximum_tokens: tightest(
                generation.maximum_tokens,
                profile.advanced.maximum_answer_tokens,
            ),
            temperature: generation
                .temperature
                .map(|value| value.clamp(0.0, profile.advanced.temperature)),
            ..generation
        };
        let known = self
            .model_thinking(&profile, &endpoint, &request, cancellation)
            .await;
        // A known model is asked as its template expects; only an unknown one learns the switch.
        let learns_switch = supports_reasoning_switch(profile.preset)
            && effort.level().is_none()
            && known.is_none();
        let key = (id, profile.model.clone(), effort);
        let mut candidates = thinking_attempts_with(&profile, known.as_ref(), effort);
        // The variant that worked for this model and effort before goes first (this session).
        if let Some(accepted) = self.accepted_thinking(&key)
            && let Some(position) = candidates.iter().position(|attempt| *attempt == accepted)
        {
            let accepted = candidates.remove(position);
            candidates.insert(0, accepted);
        }
        let request = if effort.level().is_some() {
            // A thinking model may stay silent for long between chunks.
            LlmProbeRequest {
                timeout: request.timeout.max(FIRST_REPLY_MIN_WAIT),
                ..request
            }
        } else {
            request
        };
        let emitted = AtomicBool::new(false);
        let mut report = |fragment: &str| {
            emitted.store(true, Ordering::Relaxed);
            on_delta(fragment);
        };
        for candidate in candidates {
            let attempt = request.with_thinking(candidate);
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
            if learns_switch {
                let learned = if reasoning_seen {
                    candidate.switch.after_ineffective()
                } else {
                    candidate.switch
                };
                self.remember_reasoning_switch(id, learned);
            } else if candidate.asks_anything() || effort != ThinkingEffort::Auto {
                self.remember_thinking(key.clone(), candidate);
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

    /// What is known about how `profile`'s model thinks (task 0362): learned from the template an
    /// oMLX server on this machine uses, else the curated list, else its family. The template is
    /// looked at once per session; an unchanged one reuses what was learned before.
    async fn model_thinking(
        &self,
        profile: &LlmProfile,
        endpoint: &NormalizedEndpoint,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Option<crate::model_thinking::ModelThinking> {
        if profile.preset == LlmPreset::Omlx && endpoint.loopback {
            let key = (endpoint.base_url.clone(), profile.model.clone());
            let first_look = self
                .templates_checked
                .lock()
                .map(|mut checked| checked.insert(key.clone()))
                .unwrap_or(false);
            if first_look
                && let Ok(Some(template)) =
                    self.transport.chat_template(request, cancellation).await
            {
                self.learn_template(&key.0, &key.1, &template);
            }
            if let Some(entry) = self.learned_thinking.get(&key.0, &key.1) {
                return Some(entry.thinking);
            }
        }
        crate::model_thinking::lookup(&profile.model).cloned()
    }

    /// Analyses `template` unless it is the one already learned for `model` on `endpoint`.
    fn learn_template(&self, endpoint: &str, model: &str, template: &str) {
        let hash = crate::model_thinking::learned::template_hash(template);
        if self
            .learned_thinking
            .get(endpoint, model)
            .is_some_and(|entry| entry.template_sha256 == hash)
        {
            return;
        }
        if let Some(thinking) = crate::model_thinking::template::analyse(template) {
            self.learned_thinking
                .put(endpoint, model, template, thinking);
        }
    }

    /// The thinking attempt an endpoint accepted for a model and effort in this session.
    fn accepted_thinking(&self, key: &ThinkingKey) -> Option<ThinkingAttempt> {
        self.accepted_thinking
            .lock()
            .ok()
            .and_then(|accepted| accepted.get(key).copied())
    }

    fn remember_thinking(&self, key: ThinkingKey, attempt: ThinkingAttempt) {
        if let Ok(mut accepted) = self.accepted_thinking.lock() {
            accepted.insert(key, attempt);
        }
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

    /// Whether chat requests for `role` carry native tools: the probe found the model takes them.
    /// The context window, in tokens, of the role's profile.
    pub(crate) fn role_context_window(&self, role: LlmRole) -> Option<u32> {
        self.resolve_role(role)
            .ok()
            .map(|resolved| resolved.profile.advanced.context_window)
    }

    pub(crate) async fn role_uses_native_tools(&self, role: LlmRole) -> bool {
        let Ok(resolved) = self.resolve_role(role) else {
            return false;
        };
        let profile = LlmProfile {
            model: resolved.model,
            ..resolved.profile
        };
        if !profile.takes_native_tools() {
            // Not probed yet for this model (an older profile, or a role's own model): find out in
            // the background; this request uses the text protocol.
            if profile
                .tool_calling
                .as_ref()
                .is_none_or(|probe| probe.model != profile.model)
            {
                self.schedule_tool_probe(profile.id, profile.model.clone());
            }
            return false;
        }
        true
    }

    /// Finds out in the background whether `model` takes native tool calls, and saves the answer on
    /// the profile. Runs when an endpoint or model is new or changed, never blocks the caller, and
    /// asks again only after [`TOOL_PROBE_RETRY`] when the endpoint could not be reached or has not
    /// consented. Does nothing outside a Tokio runtime.
    fn schedule_tool_probe(&self, id: Uuid, model: String) {
        // Tests call `probe_tool_calling` themselves: a background probe would race their fakes.
        if cfg!(test) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        {
            let mut probing = self
                .probing
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let now = Instant::now();
            if probing
                .get(&(id, model.clone()))
                .is_some_and(|not_before| *not_before > now)
            {
                return;
            }
            // Held until the probe finishes; the retry time replaces it on failure.
            probing.insert((id, model.clone()), now + TOOL_PROBE_RETRY);
        }
        let service = self.clone();
        runtime.spawn(async move {
            let key = (id, model.clone());
            let recorded = service.probe_tool_calling(id, &model).await;
            let mut probing = service
                .probing
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if recorded {
                probing.remove(&key);
            } else {
                probing.insert(key, Instant::now() + TOOL_PROBE_RETRY);
            }
        });
    }

    /// Asks the model to call a harmless tool and records whether it did. Returns whether an answer
    /// was recorded: an unreachable endpoint, a missing consent or a rejected request record nothing.
    async fn probe_tool_calling(&self, id: Uuid, model: &str) -> bool {
        let Ok(mut profile) = self.profile(id) else {
            return false;
        };
        profile.model = model.to_owned();
        let Ok(endpoint) = normalize_endpoint(&profile.base_url) else {
            return false;
        };
        if self
            .enforce_policy(&endpoint, profile.advanced.tls_policy)
            .is_err()
            || (endpoint.locality == EndpointLocality::Cloud
                && profile.consented_host.as_deref() != Some(endpoint.base_url.as_str()))
        {
            return false;
        }
        let Ok(request) = self.probe_request(&profile).await else {
            return false;
        };
        if request.ollama_generation_url.is_some() {
            // Ollama reports it per model, so nothing needs to be tried.
            return match self
                .transport
                .model_capabilities(&request, &CancellationToken::new())
                .await
            {
                Ok(capabilities) => {
                    self.record_tool_calling(id, model, &endpoint, capabilities.tools)
                }
                Err(_) => false,
            };
        }
        let probe = LlmChatGeneration {
            followups: Vec::new(),
            tools: vec![ToolSpec {
                name: "echo",
                description: "Repeats the given text.",
                schema: json!({
                    "type": "object",
                    "properties": {"text": {"type": "string"}},
                    "required": ["text"],
                }),
            }],
            system_prompt: "You are checking that tool calls work. Always answer by calling the \
                            echo tool."
                .to_owned(),
            user_prompt: "Call the echo tool with the text ping.".to_owned(),
            images: Vec::new(),
            // No cap: a model that reasons first must not be cut off before its call, which would
            // record the endpoint as unable to call tools.
            maximum_tokens: None,
            context_window: None,
            temperature: Some(0.0),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
        };
        let cancellation = CancellationToken::new();
        let outcome = self
            .transport
            .generate_chat_streamed(&request, &probe, &cancellation, &mut |_: &str| {})
            .await;
        let supported = match outcome {
            Ok(ChatOutcome::Generated { text, .. }) => text
                .lines()
                .any(|line| line.trim_start().starts_with("TOOL ") && line.contains("\"echo\"")),
            _ => return false,
        };
        self.record_tool_calling(id, model, &endpoint, supported)
    }

    /// Saves what the probe found, unless the endpoint changed while it ran.
    fn record_tool_calling(
        &self,
        id: Uuid,
        model: &str,
        endpoint: &NormalizedEndpoint,
        supported: bool,
    ) -> bool {
        let Ok(mut document) = self.lock() else {
            return false;
        };
        let Some(stored) = document
            .profiles
            .iter_mut()
            .find(|candidate| candidate.id == id)
        else {
            return false;
        };
        // The endpoint changed while the probe ran: the answer is about another server.
        if normalize_endpoint(&stored.base_url).map(|current| current.base_url)
            != Ok(endpoint.base_url.clone())
        {
            return false;
        }
        stored.tool_calling = Some(ToolCallingProbe {
            model: model.to_owned(),
            supported,
        });
        let _ = self.persist(&document);
        true
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
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
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
        tool_calling: None,
    }
}

fn validate_profile(profile: &LlmProfile) -> Result<(), LlmProfileError> {
    if profile.name.trim().is_empty()
        || profile.model.trim().is_empty()
        || profile.advanced.context_window == 0
        || profile
            .advanced
            .maximum_answer_tokens
            .is_some_and(|maximum| maximum == 0 || maximum > profile.advanced.context_window)
        || profile.advanced.context_window > MAX_GENERATION_CONTEXT_WINDOW
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

/// The folder oMLX's `/v1/models/status` names for `model`.
fn model_folder(status: &[u8], model: &str) -> Option<std::path::PathBuf> {
    let value: Value = serde_json::from_slice(status).ok()?;
    value
        .get("models")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(model))?
        .get("model_path")?
        .as_str()
        .map(std::path::PathBuf::from)
}

/// Longest `tokenizer_config.json` read for its template (it may list many added tokens).
const MAX_TOKENIZER_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

/// The chat template in a model folder: `chat_template.jinja`, else the one in
/// `tokenizer_config.json`. Bounded in size; anything unreadable is `None`.
async fn read_chat_template(folder: &std::path::Path) -> Option<String> {
    let jinja = folder.join("chat_template.jinja");
    if let Ok(metadata) = tokio::fs::metadata(&jinja).await
        && metadata.is_file()
        && metadata.len() <= crate::model_thinking::template::MAX_TEMPLATE_BYTES as u64
    {
        return tokio::fs::read_to_string(&jinja).await.ok();
    }
    let config = folder.join("tokenizer_config.json");
    let metadata = tokio::fs::metadata(&config).await.ok()?;
    if !metadata.is_file() || metadata.len() > MAX_TOKENIZER_CONFIG_BYTES {
        return None;
    }
    template_from_tokenizer_config(&tokio::fs::read(&config).await.ok()?)
}

/// The `chat_template` of a `tokenizer_config.json`: a string, or the `default` of a named list.
pub(crate) fn template_from_tokenizer_config(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    match value.get("chat_template")? {
        Value::String(template) => Some(template.clone()),
        Value::Array(named) => named
            .iter()
            .find(|entry| entry.get("name").and_then(Value::as_str) == Some("default"))
            .and_then(|entry| entry.get("template")?.as_str().map(str::to_owned)),
        _ => None,
    }
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
    let mut messages = vec![json!({"role": "system", "content": generation.system_prompt})];
    if !generation.user_prompt.is_empty() || !generation.images.is_empty() {
        messages.push(json!({"role": "user", "content": openai_user_content(generation)}));
    }
    messages.extend(generation.followups.iter().map(openai_message));
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
    });
    if let Some(maximum) = generation.maximum_tokens {
        body["max_tokens"] = json!(maximum);
    }
    if let Some(temperature) = generation.temperature {
        body["temperature"] = json!(temperature);
    }
    if !generation.tools.is_empty() {
        body["tools"] = tool_definitions(&generation.tools);
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
    if let Some(effort) = request.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    if let Some((name, on)) = request.template_switch {
        body["chat_template_kwargs"][name] = json!(on);
    }
    body
}

/// One conversation message in the OpenAI chat format: a reply carries its calls as `tool_calls`
/// (arguments as a JSON string), a result is a `tool` message naming the call's id.
fn openai_message(message: &Message) -> Value {
    match message {
        Message::User(text) | Message::Note(text) => json!({"role": "user", "content": text}),
        Message::Assistant { text, calls } => {
            let prose = Message::prose(text);
            let mut value = json!({"role": "assistant", "content": prose});
            if !calls.is_empty() {
                if prose.is_empty() {
                    value["content"] = Value::Null;
                }
                value["tool_calls"] = calls
                    .iter()
                    .map(|call| {
                        json!({"id": call.id, "type": "function", "function": {
                            "name": call.name,
                            "arguments": call.arguments.to_string(),
                        }})
                    })
                    .collect();
            }
            value
        }
        Message::ToolResult { call_id, text, .. } => {
            json!({"role": "tool", "tool_call_id": call_id, "content": text})
        }
    }
}

/// One conversation message in Ollama's chat format: calls carry their arguments as an object,
/// and a result names its tool (Ollama has no call ids).
fn ollama_message(message: &Message) -> Value {
    match message {
        Message::User(text) | Message::Note(text) => json!({"role": "user", "content": text}),
        Message::Assistant { text, calls } => {
            let mut value = json!({"role": "assistant", "content": Message::prose(text)});
            if !calls.is_empty() {
                value["tool_calls"] = calls
                    .iter()
                    .map(|call| {
                        json!({"function": {"name": call.name, "arguments": call.arguments}})
                    })
                    .collect();
            }
            value
        }
        Message::ToolResult { name, text, .. } => {
            json!({"role": "tool", "content": text, "tool_name": name})
        }
    }
}

/// The `tools` list OpenAI-compatible endpoints and Ollama take.
fn tool_definitions(tools: &[ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({"type": "function", "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.schema,
                }})
            })
            .collect(),
    )
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
    let mut options = json!({});
    if let Some(maximum) = generation.maximum_tokens {
        options["num_predict"] = json!(maximum);
    }
    if let Some(temperature) = generation.temperature {
        options["temperature"] = json!(temperature);
    }
    if let Some(context_window) = generation.context_window {
        options["num_ctx"] = Value::from(context_window);
    }
    let mut messages = vec![json!({"role": "system", "content": generation.system_prompt})];
    if !generation.user_prompt.is_empty() || !images.is_empty() {
        messages.push(json!({"role": "user", "content": generation.user_prompt, "images": images}));
    }
    messages.extend(generation.followups.iter().map(ollama_message));
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": false,
        "think": request.ollama_think.value(),
        "options": options
    });
    if !generation.tools.is_empty() {
        // A JSON-only reply could not be a tool call, so tools replace the JSON constraint.
        body["tools"] = tool_definitions(&generation.tools);
    } else if generation.reply == ReplyFormat::Json {
        body["format"] = json!("json");
    }
    body
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
        tools: capabilities
            .iter()
            .any(|value| value.as_str() == Some("tools")),
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
            .map(|value| value.min(MAX_GENERATION_CONTEXT_WINDOW)),
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

/// A complete reply's text; a reply cut off at a token limit is an error here.
fn parse_streaming_chat(bytes: &[u8]) -> Result<String, LlmProfileError> {
    let reply = parse_streaming_chat_detailed(bytes)?;
    if reply.cut_off {
        return Err(LlmProfileError::ReplyCutOff);
    }
    Ok(reply.text)
}

/// A parsed reply, before the caller decides what a cut-off one is worth.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedReply {
    text: String,
    /// The model also produced reasoning (`reasoning_content`/`reasoning`, or `<think>` tags).
    reasoning_seen: bool,
    /// The model stopped at a token limit; `text` is what it wrote until then, without any
    /// native call (a call's arguments may be incomplete).
    cut_off: bool,
}

/// Note added to a prose reply that a token limit cut off.
const CUT_OFF_NOTE: &str = "\n\n[The answer was cut off at the model's token limit.]";

/// The reply text for a caller: a cut-off prose reply is kept with a note, since most of an answer
/// is worth more than asking again; a cut-off JSON reply is an error, since half a value is useless.
fn finish_reply(reply: ParsedReply, format: ReplyFormat) -> Result<String, LlmProfileError> {
    match (reply.cut_off, format) {
        (false, _) => Ok(reply.text),
        (true, ReplyFormat::Json) => Err(LlmProfileError::ReplyCutOff),
        (true, ReplyFormat::Text) if reply.text.trim().is_empty() => {
            Err(LlmProfileError::ReplyCutOff)
        }
        (true, ReplyFormat::Text) => {
            tracing::warn!("a reply was cut off at the model's token limit; kept with a note");
            Ok(format!("{}{CUT_OFF_NOTE}", reply.text.trim_end()))
        }
    }
}

/// Parses a streamed completion: its content, whether the model also streamed reasoning, and
/// whether it stopped at a token limit.
fn parse_streaming_chat_detailed(bytes: &[u8]) -> Result<ParsedReply, LlmProfileError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LlmProfileError::MalformedResponse)?;
    let mut valid_event = false;
    let mut terminated = false;
    let mut truncated = false;
    let mut content = String::new();
    let mut reasoning_seen = false;
    // Native tool calls arrive in pieces: an id and a name, then argument fragments, by call index.
    let mut tool_calls: Vec<StreamedCall> = Vec::new();
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
                if content.len() > MAX_REPLY_BYTES {
                    return Err(LlmProfileError::MalformedResponse);
                }
            }
        }
        for choice in choices {
            let Some(calls) = choice
                .get("delta")
                .and_then(|delta| delta.get("tool_calls"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for call in calls {
                let index = call
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .unwrap_or(0)
                    .min(MAX_TOOL_CALLS_PER_REPLY);
                if tool_calls.len() <= index {
                    tool_calls.resize_with(index + 1, StreamedCall::default);
                }
                if let Some(id) = call.get("id").and_then(Value::as_str)
                    && tool_calls[index].id.is_empty()
                {
                    tool_calls[index].id = id.chars().take(MAX_CALL_ID_CHARS).collect();
                }
                let function = call.get("function");
                if let Some(name) = function.and_then(|f| f.get("name")).and_then(Value::as_str) {
                    tool_calls[index].name.push_str(name);
                }
                if let Some(arguments) = function
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                {
                    tool_calls[index].arguments.push_str(arguments);
                    if tool_calls[index].arguments.len() > MAX_REPLY_BYTES {
                        return Err(LlmProfileError::MalformedResponse);
                    }
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
    if valid_event && terminated {
        // Every named call becomes a `TOOL {json}` line; the agent loop runs them in order. A
        // cut-off reply runs none: its last call's arguments may be incomplete.
        let lines: Vec<String> = tool_calls
            .iter()
            .filter(|_| !truncated)
            .filter_map(|call| tool_call_line(Some(&call.id), &call.name, &call.arguments))
            .collect();
        if !lines.is_empty() {
            let line = lines.join("\n");
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&line);
        }
        reasoning_seen |= content.contains("<think>") || content.contains("</think>");
        Ok(ParsedReply {
            text: content,
            reasoning_seen,
            cut_off: truncated,
        })
    } else {
        Err(LlmProfileError::MalformedResponse)
    }
}

/// How long after a probe that recorded nothing the next one may start.
const TOOL_PROBE_RETRY: Duration = Duration::from_secs(10 * 60);

/// Most tool calls looked at in one streamed reply.
const MAX_TOOL_CALLS_PER_REPLY: usize = 16;
/// Longest call id kept from an endpoint.
const MAX_CALL_ID_CHARS: usize = 128;

/// One native call as it streams in.
#[derive(Debug, Clone, Default)]
struct StreamedCall {
    id: String,
    name: String,
    arguments: String,
}

/// A native tool call as a text-protocol line (`TOOL {"id":..,"name":..,"arguments":{..}}`), so
/// the agent loop reads native and text calls the same way and can answer the call by its id.
/// Malformed arguments are kept as a string so the harness reports the error to the model.
fn tool_call_line(id: Option<&str>, name: &str, arguments: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let arguments = fm_agent::repair_tool_arguments(arguments)
        .unwrap_or_else(|| Value::String(arguments.to_owned()));
    Some(fm_agent::tool_call_line(id, name, &arguments))
}

/// A complete Ollama reply's text; one cut off at a token limit is an error here.
#[cfg(test)]
fn parse_ollama_chat(bytes: &[u8]) -> Result<String, LlmProfileError> {
    let reply = parse_ollama_reply(bytes)?;
    if reply.cut_off {
        return Err(LlmProfileError::ReplyCutOff);
    }
    Ok(reply.text)
}

fn parse_ollama_reply(bytes: &[u8]) -> Result<ParsedReply, LlmProfileError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| LlmProfileError::MalformedResponse)?;
    if value.get("done").and_then(Value::as_bool) != Some(true) {
        return Err(LlmProfileError::MalformedResponse);
    }
    let cut_off = value.get("done_reason").and_then(Value::as_str) == Some("length");
    let message = value.get("message");
    // Native tool calls come back as text-protocol lines, one per call (task 0339).
    let lines: Vec<String> = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
        // A cut-off reply runs no call: its arguments may be incomplete.
        .filter(|_| !cut_off)
        .map(|calls| {
            calls
                .iter()
                .take(MAX_TOOL_CALLS_PER_REPLY)
                .filter_map(|call| {
                    let function = call.get("function")?;
                    let name = function.get("name")?.as_str()?;
                    let arguments = function
                        .get("arguments")
                        .map(Value::to_string)
                        .unwrap_or_default();
                    let id = call.get("id").and_then(Value::as_str);
                    tool_call_line(id, name, &arguments)
                })
                .collect()
        })
        .unwrap_or_default();
    let text = if lines.is_empty() {
        message
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .filter(|content| !content.trim().is_empty())
            .map(str::to_owned)
            .ok_or(if cut_off {
                LlmProfileError::ReplyCutOff
            } else {
                LlmProfileError::MalformedResponse
            })?
    } else {
        lines.join("\n")
    };
    Ok(ParsedReply {
        text,
        reasoning_seen: false,
        cut_off,
    })
}

/// Most bytes of a refusal's body written to the log.
const MAX_LOGGED_REFUSAL: usize = 2048;

/// The response when its status is a success. Otherwise the endpoint's own explanation (a context
/// that is too long, an unsupported parameter) is logged, since the person only sees that the
/// assistant is unavailable.
async fn refused_generation(
    mut response: reqwest::Response,
) -> Result<reqwest::Response, LlmProfileError> {
    let status = response.status().as_u16();
    let Err(error) = classify_http_status(status) else {
        return Ok(response);
    };
    let mut body = Vec::new();
    while body.len() < MAX_LOGGED_REFUSAL {
        match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
            Ok(Ok(Some(chunk))) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    body.truncate(MAX_LOGGED_REFUSAL);
    let body = fm_agent::redact_secrets(&String::from_utf8_lossy(&body));
    tracing::warn!(status, body = %body, "the chat endpoint refused the request");
    Err(error)
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

/// The tighter of two optional token caps; `None` when neither is set.
fn tightest(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
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
    /// The model stopped at its token limit (`finish_reason: "length"`), so the reply is cut off.
    /// A model that reasons first may spend the whole limit before it writes.
    #[error("the model's reply was cut off at its token limit")]
    ReplyCutOff,
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
            Self::MalformedResponse | Self::ReplyCutOff => LlmTestErrorCategory::MalformedResponse,
            Self::Cancelled => LlmTestErrorCategory::Cancelled,
            Self::Transport => LlmTestErrorCategory::Transport,
        }
    }
}

// --- Conversions (folded from `llm_profile_mapping`) ---

fn assignment_to_dto(
    assignment: RoleAssignment,
    thinking_support: Option<fm_transport_dto::ThinkingSupportDto>,
) -> LlmRoleAssignmentDto {
    LlmRoleAssignmentDto {
        thinking_support,
        profile_id: assignment.profile_id,
        model: assignment.model,
        allow_cloud: assignment.allow_cloud,
        thinking: thinking_to_dto(assignment.thinking),
    }
}

/// What a model supports for thinking, for Settings: `learned` when its template was read on this
/// machine, else the curated list or its family.
fn thinking_support_dto(
    entry: &crate::model_thinking::ModelThinking,
    learned: Option<&crate::model_thinking::learned::LearnedEntry>,
) -> fm_transport_dto::ThinkingSupportDto {
    use crate::model_thinking::{LevelName, ThinkingKind};
    use fm_transport_dto::ThinkingOriginDto;
    let levels = [
        (LevelName::Low, ThinkingEffortDto::Low),
        (LevelName::Medium, ThinkingEffortDto::Medium),
        (LevelName::High, ThinkingEffortDto::High),
    ]
    .into_iter()
    .filter(|(level, _)| entry.level(*level).is_some())
    .map(|(_, dto)| dto)
    .collect();
    let origin = match (learned, entry.family) {
        (Some(_), _) => ThinkingOriginDto::Template,
        (None, true) => ThinkingOriginDto::Family,
        (None, false) => ThinkingOriginDto::Card,
    };
    fm_transport_dto::ThinkingSupportDto {
        thinks: entry.thinking != ThinkingKind::None,
        can_turn_off: entry.thinking == ThinkingKind::Toggle,
        default_on: entry.default_on,
        levels,
        verified: !entry.family,
        source: entry.source.clone(),
        note: entry.note.clone(),
        origin,
        learned_at: learned.map(|learned| learned.learned_at.clone()),
        list_entry: learned.and_then(list_entry),
    }
}

/// A learned entry as it would go into `model_thinking.json`.
fn list_entry(learned: &crate::model_thinking::learned::LearnedEntry) -> Option<String> {
    let mut entry = learned.thinking.clone();
    entry.matches = vec![learned.model.to_ascii_lowercase()];
    entry.source = learned.model.clone();
    entry.tested = vec![format!(
        "Read from the chat template {} serves ({}).",
        learned.endpoint,
        learned.learned_at.get(..10).unwrap_or(&learned.learned_at)
    )];
    serde_json::to_string_pretty(&entry).ok()
}

pub(crate) fn thinking_to_dto(effort: ThinkingEffort) -> ThinkingEffortDto {
    match effort {
        ThinkingEffort::Auto => ThinkingEffortDto::Auto,
        ThinkingEffort::Off => ThinkingEffortDto::Off,
        ThinkingEffort::Low => ThinkingEffortDto::Low,
        ThinkingEffort::Medium => ThinkingEffortDto::Medium,
        ThinkingEffort::High => ThinkingEffortDto::High,
    }
}

pub(crate) fn thinking_from_dto(effort: ThinkingEffortDto) -> ThinkingEffort {
    match effort {
        ThinkingEffortDto::Auto => ThinkingEffort::Auto,
        ThinkingEffortDto::Off => ThinkingEffort::Off,
        ThinkingEffortDto::Low => ThinkingEffort::Low,
        ThinkingEffortDto::Medium => ThinkingEffort::Medium,
        ThinkingEffortDto::High => ThinkingEffort::High,
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
        thinking: request.thinking.map(thinking_from_dto),
    }
}

pub(crate) fn model_limits_to_dto(limits: LlmModelLimits) -> LlmModelLimitsDto {
    LlmModelLimitsDto {
        context_window: limits.context_window,
        maximum_answer_tokens: limits.maximum_answer_tokens,
        context_window_cap: MAX_GENERATION_CONTEXT_WINDOW,
        maximum_answer_tokens_cap: MAX_GENERATION_CONTEXT_WINDOW,
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
        tool_calling: profile
            .tool_calling
            .map(|probe| fm_transport_dto::ToolCallingProbeDto {
                model: probe.model,
                supported: probe.supported,
            }),
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
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let generation = LlmChatGeneration {
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "s".into(),
            user_prompt: "u".into(),
            images: Vec::new(),
            maximum_tokens: Some(10),
            context_window: None,
            temperature: Some(0.0),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
        };
        let off = openai_generation_body(&request, &generation, ReasoningSwitch::Both);
        assert_eq!(off["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(off["reasoning_effort"], "none");
        let plain = openai_generation_body(&request, &generation, ReasoningSwitch::Plain);
        assert!(plain.get("chat_template_kwargs").is_none());
        assert!(plain.get("reasoning_effort").is_none());
        assert!(plain.get("tools").is_none());

        let with_tools = LlmChatGeneration {
            followups: Vec::new(),
            tools: vec![ToolSpec {
                name: "read_file",
                description: "reads a file",
                schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }],
            ..generation
        };
        let body = openai_generation_body(&request, &with_tools, ReasoningSwitch::Plain);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn streamed_native_tool_calls_become_a_text_protocol_line() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Let me look.\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let text = parse_streaming_chat(stream.as_bytes()).unwrap();
        assert_eq!(
            text,
            "Let me look.\nTOOL {\"id\":\"c1\",\"name\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}"
        );
        // Invalid JSON reaches the turn loop and is rejected there.
        assert_eq!(
            tool_call_line(None, "list_dir", "not json").as_deref(),
            Some("TOOL {\"name\":\"list_dir\",\"arguments\":\"not json\"}")
        );
        assert_eq!(
            tool_call_line(Some("c2"), "read_file", "{filePath:'a.txt',}").as_deref(),
            Some(
                "TOOL {\"id\":\"c2\",\"name\":\"read_file\",\"arguments\":{\"filePath\":\"a.txt\"}}"
            )
        );
        assert_eq!(tool_call_line(None, "", "{}"), None);
    }

    #[test]
    fn every_streamed_native_call_becomes_its_own_line() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"list_dir\",\"arguments\":\"{}\"}},{\"index\":1,\"function\":{\"name\":\"grep\",\"arguments\":\"{\\\"pattern\\\":\\\"x\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        assert_eq!(
            parse_streaming_chat(stream.as_bytes()).unwrap(),
            "TOOL {\"name\":\"list_dir\",\"arguments\":{}}\nTOOL {\"name\":\"grep\",\"arguments\":{\"pattern\":\"x\"}}"
        );
    }

    fn probe_request_for_tests() -> LlmProbeRequest {
        LlmProbeRequest {
            url: "http://localhost:8000/v1/chat/completions".into(),
            ollama_generation_url: None,
            model: "m".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Plain,
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        }
    }

    fn conversation_with_a_call() -> Vec<Message> {
        let call = fm_agent::ToolCallRecord {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a.txt"}),
        };
        vec![
            Message::Assistant {
                text: "Let me look.\nTOOL {\"id\":\"c1\",\"name\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}".into(),
                calls: vec![call.clone()],
            },
            Message::result(&call, "alpha"),
            Message::Note("Go on.".into()),
        ]
    }

    #[test]
    fn openai_bodies_carry_calls_and_results_as_tool_messages() {
        let generation = LlmChatGeneration {
            followups: conversation_with_a_call(),
            ..tiny_generation()
        };
        let body = openai_generation_body(
            &probe_request_for_tests(),
            &generation,
            ReasoningSwitch::Plain,
        );
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "Let me look.");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(messages[2]["tool_calls"][0]["type"], "function");
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.txt\"}"
        );
        assert_eq!(
            messages[3],
            json!({"role": "tool", "tool_call_id": "c1", "content": "alpha"})
        );
        assert_eq!(messages[4], json!({"role": "user", "content": "Go on."}));
    }

    #[test]
    fn ollama_bodies_carry_calls_and_results_by_tool_name() {
        let generation = LlmChatGeneration {
            followups: conversation_with_a_call(),
            tools: vec![ToolSpec {
                name: "read_file",
                description: "Reads.",
                schema: json!({"type": "object"}),
            }],
            ..tiny_generation()
        };
        let body = ollama_generation_body(&probe_request_for_tests(), &generation);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(
            messages[2]["tool_calls"][0]["function"],
            json!({"name": "read_file", "arguments": {"path": "a.txt"}})
        );
        assert_eq!(
            messages[3],
            json!({"role": "tool", "content": "alpha", "tool_name": "read_file"})
        );
    }

    #[test]
    fn a_native_call_with_only_calls_has_no_text_content() {
        let call = fm_agent::ToolCallRecord {
            id: "c9".into(),
            name: "list_dir".into(),
            arguments: json!({}),
        };
        let message = openai_message(&Message::Assistant {
            text: "TOOL {\"id\":\"c9\",\"name\":\"list_dir\",\"arguments\":{}}".into(),
            calls: vec![call],
        });
        assert_eq!(message["content"], Value::Null);
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
        /// The tools each generation carried.
        generated_tools: Mutex<Vec<Vec<ToolSpec>>>,
        /// The prompt and the number of follow-up messages each generation carried.
        generated_prompts: Mutex<Vec<(String, usize)>>,
        /// Whether a generation offered the `echo` probe tool is answered with a call to it.
        script_tool_call: std::sync::atomic::AtomicBool,
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
            generation: &LlmChatGeneration,
            cancellation: &CancellationToken,
        ) -> Result<String, LlmProfileError> {
            if cancellation.is_cancelled() {
                return Err(LlmProfileError::Cancelled);
            }
            self.generated_tools
                .lock()
                .unwrap()
                .push(generation.tools.clone());
            self.generated_prompts
                .lock()
                .unwrap()
                .push((generation.user_prompt.clone(), generation.followups.len()));
            self.generate_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(failure) = self.generate_failures.lock().unwrap().pop_front() {
                return Err(failure);
            }
            self.captured.lock().unwrap().push(request.clone());
            if self
                .script_tool_call
                .load(std::sync::atomic::Ordering::Relaxed)
                && generation.tools.iter().any(|tool| tool.name == "echo")
            {
                return Ok(r#"TOOL {"tool":"echo","text":"ping"}"#.to_owned());
            }
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
            generated_tools: Mutex::new(Vec::new()),
            generated_prompts: Mutex::new(Vec::new()),
            script_tool_call: std::sync::atomic::AtomicBool::new(false),
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
                maximum_answer_tokens: Some(50_000),
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
        // No answer limit is the default and valid; a set one is bounded by the context window.
        assert!(validate_profile(&profile).is_ok());
        profile.advanced.context_window = MAX_GENERATION_CONTEXT_WINDOW;
        profile.advanced.maximum_answer_tokens = Some(MAX_GENERATION_CONTEXT_WINDOW);
        assert!(validate_profile(&profile).is_ok());
        profile.advanced.context_window += 1;
        assert!(matches!(
            validate_profile(&profile),
            Err(LlmProfileError::InvalidConfiguration)
        ));
        profile.advanced.context_window = 8_192;
        profile.advanced.maximum_answer_tokens = Some(8_193);
        assert!(matches!(
            validate_profile(&profile),
            Err(LlmProfileError::InvalidConfiguration)
        ));
        profile.advanced.maximum_answer_tokens = Some(0);
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
        assert_eq!(limits.maximum_answer_tokens, Some(32_000));
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
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let body = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                followups: Vec::new(),
                tools: Vec::new(),
                system_prompt: "Summarize.".into(),
                user_prompt: "Evidence".into(),
                images: vec![LlmImageAttachment {
                    media_type: "image/png".into(),
                    data: vec![1, 2, 3],
                }],
                maximum_tokens: Some(512),
                context_window: Some(8_192),
                temperature: Some(0.1),
                reply: ReplyFormat::Json,
                thinking: crate::llm_profiles::ThinkingRequest::Background,
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
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let generation = |temperature| LlmChatGeneration {
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "s".into(),
            user_prompt: "u".into(),
            images: Vec::new(),
            maximum_tokens: Some(8),
            context_window: None,
            temperature,
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
        };
        let unset = openai_generation_body(&request, &generation(None), ReasoningSwitch::Plain);
        assert!(unset.get("temperature").is_none());
        let set = openai_generation_body(&request, &generation(Some(0.2)), ReasoningSwitch::Plain);
        assert!((set["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn openai_user_content_puts_each_image_after_the_text() {
        let text_only = openai_user_content(&LlmChatGeneration {
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "s".into(),
            user_prompt: "Describe".into(),
            images: Vec::new(),
            maximum_tokens: Some(8),
            context_window: None,
            temperature: Some(0.0),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
        });
        assert_eq!(text_only, json!("Describe"));

        let with_image = openai_user_content(&LlmChatGeneration {
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "s".into(),
            user_prompt: "Describe".into(),
            images: vec![LlmImageAttachment {
                media_type: "image/png".into(),
                data: vec![1, 2, 3],
            }],
            maximum_tokens: Some(8),
            context_window: None,
            temperature: Some(0.0),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
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
            thinking: ThinkingEffort::Auto,
        };
        assert_eq!(document.roles.chat, Some(expected.clone()));
        assert_eq!(document.roles.embeddings, Some(expected));
    }

    #[test]
    fn version_five_turns_only_the_old_default_answer_cap_into_no_cap() {
        let mut default_capped = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        default_capped.advanced.maximum_answer_tokens = Some(1_024);
        let mut chosen = default_capped.clone();
        chosen.id = Uuid::new_v4();
        chosen.advanced.maximum_answer_tokens = Some(4_000);
        let value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 5,
            active_profile_id: None,
            profiles: vec![default_capped, chosen],
            roles: LlmRoles::default(),
        })
        .unwrap();

        let migrated = LlmProfilesDocument::migrate(value, 5).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();

        document.validate().unwrap();
        let caps: Vec<Option<u32>> = document
            .profiles
            .iter()
            .map(|profile| profile.advanced.maximum_answer_tokens)
            .collect();
        assert_eq!(caps, [None, Some(4_000)]);
    }

    #[test]
    fn no_cap_is_sent_unless_one_is_set() {
        let request = probe_request_for_tests();
        let uncapped = LlmChatGeneration {
            maximum_tokens: None,
            ..tiny_generation()
        };
        let openai = openai_generation_body(&request, &uncapped, ReasoningSwitch::Plain);
        assert!(openai.get("max_tokens").is_none(), "{openai}");
        let ollama = ollama_generation_body(&request, &uncapped);
        assert!(ollama["options"].get("num_predict").is_none(), "{ollama}");

        let capped = openai_generation_body(&request, &tiny_generation(), ReasoningSwitch::Plain);
        assert_eq!(capped["max_tokens"], 16);
        assert_eq!(tightest(None, Some(5)), Some(5));
        assert_eq!(tightest(Some(9), Some(5)), Some(5));
        assert_eq!(tightest(None, None), None);
    }

    #[test]
    fn a_cut_off_prose_reply_is_kept_with_a_note_and_without_its_calls() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"content\":\"Most of the answer\",\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"pa\"}}]},\"finish_reason\":\"length\"}]}\n\n";
        let reply = parse_streaming_chat_detailed(stream).unwrap();
        assert!(reply.cut_off);
        assert_eq!(reply.text, "Most of the answer", "no half call is run");

        let kept = finish_reply(reply.clone(), ReplyFormat::Text).unwrap();
        assert!(kept.starts_with("Most of the answer"));
        assert!(kept.ends_with("[The answer was cut off at the model's token limit.]"));
        assert_eq!(
            finish_reply(reply, ReplyFormat::Json),
            Err(LlmProfileError::ReplyCutOff),
            "half a JSON value is useless"
        );
        let empty = ParsedReply {
            text: String::new(),
            reasoning_seen: true,
            cut_off: true,
        };
        assert_eq!(
            finish_reply(empty, ReplyFormat::Text),
            Err(LlmProfileError::ReplyCutOff)
        );

        let ollama = parse_ollama_reply(
            br#"{"message":{"role":"assistant","content":"Partial"},"done":true,"done_reason":"length"}"#,
        )
        .unwrap();
        assert!(ollama.cut_off);
        assert_eq!(ollama.text, "Partial");
    }

    #[test]
    fn a_long_streamed_reply_is_not_mistaken_for_a_runaway_one() {
        // Thousands of small events: far more raw bytes than text, as real streams send.
        let mut stream = String::new();
        for index in 0..5_000 {
            stream.push_str(&format!(
                "data: {{\"id\":\"chatcmpl-{index}\",\"object\":\"chat.completion.chunk\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"word \"}}}}]}}\n\n"
            ));
        }
        stream.push_str(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        );
        assert!(stream.len() > 256 * 1024);
        assert!(stream.len() < MAX_STREAM_BODY_BYTES);
        let text = parse_streaming_chat(stream.as_bytes()).unwrap();
        assert_eq!(text.len(), 5_000 * 5);
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
                thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
            thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
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
                    thinking: None,
                }),
            )
            .unwrap();
        assert_eq!(
            service.resolve_role(LlmRole::Chat).unwrap().profile.id,
            profile.id
        );
    }

    #[tokio::test]
    async fn the_chat_roles_default_thinking_effort_is_kept_when_a_save_names_none() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::Ollama, "http://localhost:11434"))
            .await
            .unwrap();
        let input = |thinking| RoleAssignmentInput {
            profile_id: profile.id,
            model: profile.model.clone(),
            allow_cloud: false,
            thinking,
        };
        service
            .set_role(LlmRole::Chat, Some(input(Some(ThinkingEffort::High))))
            .unwrap();
        // `/model` switches the model without naming an effort.
        service.set_role(LlmRole::Chat, Some(input(None))).unwrap();
        assert_eq!(
            service.resolve_role(LlmRole::Chat).unwrap().thinking,
            ThinkingEffort::High
        );
        service
            .set_role(LlmRole::Chat, Some(input(Some(ThinkingEffort::Off))))
            .unwrap();
        assert_eq!(
            service.resolve_role(LlmRole::Chat).unwrap().thinking,
            ThinkingEffort::Off
        );
    }

    #[test]
    fn a_conversation_choice_wins_and_background_calls_stay_auto() {
        let high = ThinkingEffort::High;
        assert_eq!(
            ThinkingRequest::Background.effort(high),
            ThinkingEffort::Auto
        );
        assert_eq!(ThinkingRequest::Conversation(None).effort(high), high);
        assert_eq!(
            ThinkingRequest::Conversation(Some(ThinkingEffort::Low)).effort(high),
            ThinkingEffort::Low
        );
    }

    #[test]
    fn thinking_attempts_match_each_kind_of_endpoint() {
        let profile = |preset: LlmPreset, model: &str| {
            let mut profile =
                profile_from_draft(Uuid::new_v4(), draft(preset, "http://127.0.0.1:8000"), None);
            profile.model = model.to_owned();
            profile
        };
        let effort = ThinkingAttempt::effort;
        let ollama = ThinkingAttempt::ollama;
        // Ollama: a level for gpt-oss, a boolean for the rest, and off as the way back.
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::Ollama, "gpt-oss:20b"),
                ThinkingEffort::High
            ),
            [ollama(OllamaThink::Level("high")), ollama(OllamaThink::Off)]
        );
        assert_eq!(
            thinking_attempts(&profile(LlmPreset::Ollama, "qwen3:8b"), ThinkingEffort::Low),
            [ollama(OllamaThink::On), ollama(OllamaThink::Off)]
        );
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::Ollama, "qwen3:8b"),
                ThinkingEffort::Auto
            ),
            [ollama(OllamaThink::Off)]
        );
        // Local OpenAI-compatible servers: a level, else nothing; off is the learned switch.
        assert_eq!(
            thinking_attempts(&profile(LlmPreset::LmStudio, "m"), ThinkingEffort::Medium),
            [effort("medium"), ThinkingAttempt::PLAIN]
        );
        assert_eq!(
            thinking_attempts(&profile(LlmPreset::Vllm, "m"), ThinkingEffort::Off)[0],
            ThinkingAttempt::switch(ReasoningSwitch::Both)
        );
        // Cloud and generic endpoints: nothing for auto; off goes from `none` down to nothing.
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::OpenAiCompatible, "m"),
                ThinkingEffort::Auto
            ),
            [ThinkingAttempt::PLAIN]
        );
        assert_eq!(
            thinking_attempts(&profile(LlmPreset::AzureOpenAi, "m"), ThinkingEffort::Off),
            [
                effort("none"),
                effort("minimal"),
                effort("low"),
                ThinkingAttempt::PLAIN
            ]
        );
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::OpenAiCompatible, "m"),
                ThinkingEffort::High
            ),
            [effort("high"), ThinkingAttempt::PLAIN]
        );
    }

    #[test]
    fn listed_models_are_asked_the_way_their_chat_template_expects() {
        let profile = |preset: LlmPreset, model: &str| {
            let mut profile =
                profile_from_draft(Uuid::new_v4(), draft(preset, "http://127.0.0.1:8000"), None);
            profile.model = model.to_owned();
            profile
        };
        let first = |preset, model, effort| thinking_attempts(&profile(preset, model), effort)[0];
        // Qwen3.8 (and ThinkingCap on it): its highest level is `xhigh`; `high` would fail.
        let qwen = first(
            LlmPreset::Omlx,
            "ThinkingCap-Qwen3.8-27B-MLX-4bit-DWQ",
            ThinkingEffort::High,
        );
        assert_eq!(qwen.effort, Some("xhigh"));
        assert_eq!(qwen.template, Some(("enable_thinking", true)));
        // Locally, Auto keeps thinking off through the template switch, with no effort.
        let auto = first(
            LlmPreset::Omlx,
            "Qwen3.8-Flash-Next-oQ4e-mtp",
            ThinkingEffort::Auto,
        );
        assert_eq!(auto.template, Some(("enable_thinking", false)));
        assert_eq!(auto.effort, None);
        // Gemma 4 only switches; Inkling only takes an effort, and `none` is its least.
        let gemma = first(
            LlmPreset::Omlx,
            "gemma-4-26B-A4B-it-QAT-MLX-4bit",
            ThinkingEffort::Low,
        );
        assert_eq!(
            (gemma.template, gemma.effort),
            (Some(("enable_thinking", true)), None)
        );
        let inkling = first(
            LlmPreset::Omlx,
            "Inkling-2-Mini-oQ4e-mtp",
            ThinkingEffort::Off,
        );
        assert_eq!((inkling.template, inkling.effort), (None, Some("none")));
        // Every listed attempt ends with one that asks nothing.
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::Omlx, "Inkling-2-Mini"),
                ThinkingEffort::High
            ),
            [ThinkingAttempt::effort("high"), ThinkingAttempt::PLAIN]
        );
        // A family member without its own entry: the family switch with a level to try, then the
        // switch alone.
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::Omlx, "Qwen3.6-35B-A3B"),
                ThinkingEffort::High
            ),
            [
                ThinkingAttempt {
                    effort: Some("high"),
                    template: Some(("enable_thinking", true)),
                    ..ThinkingAttempt::PLAIN
                },
                ThinkingAttempt {
                    template: Some(("enable_thinking", true)),
                    ..ThinkingAttempt::PLAIN
                },
                ThinkingAttempt::PLAIN
            ]
        );
        // On Ollama a model that never thinks is not asked to.
        assert_eq!(
            thinking_attempts(
                &profile(LlmPreset::Ollama, "llama3.2:3b"),
                ThinkingEffort::High
            ),
            [ThinkingAttempt::ollama(OllamaThink::Off)]
        );
        // The template switch reaches the body next to any other kwargs.
        let request = LlmProbeRequest {
            url: "http://localhost:8000/v1/chat/completions".into(),
            ollama_generation_url: None,
            model: "m".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(5),
            reasoning_switch: ReasoningSwitch::Plain,
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        }
        .with_thinking(qwen);
        let body = openai_generation_body(&request, &tiny_generation(), request.reasoning_switch);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], true);
        assert_eq!(body["reasoning_effort"], "xhigh");
    }

    #[test]
    fn the_request_bodies_carry_the_thinking_attempt() {
        let mut request = LlmProbeRequest {
            url: "http://localhost:8000/v1/chat/completions".into(),
            ollama_generation_url: None,
            model: "m".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(5),
            reasoning_switch: ReasoningSwitch::Plain,
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let plain = openai_generation_body(&request, &tiny_generation(), ReasoningSwitch::Plain);
        assert!(plain.get("reasoning_effort").is_none());
        request = request.with_thinking(ThinkingAttempt::effort("high"));
        let high = openai_generation_body(&request, &tiny_generation(), request.reasoning_switch);
        assert_eq!(high["reasoning_effort"], "high");
        assert!(high.get("chat_template_kwargs").is_none());
        request = request.with_thinking(ThinkingAttempt::ollama(OllamaThink::Level("low")));
        assert_eq!(
            ollama_generation_body(&request, &tiny_generation())["think"],
            "low"
        );
        request = request.with_thinking(ThinkingAttempt::ollama(OllamaThink::On));
        assert_eq!(
            ollama_generation_body(&request, &tiny_generation())["think"],
            true
        );
        request = request.with_thinking(ThinkingAttempt::PLAIN);
        assert_eq!(
            ollama_generation_body(&request, &tiny_generation())["think"],
            false
        );
    }

    #[tokio::test]
    async fn the_template_is_found_in_the_folder_omlx_names_for_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("org/model-a");
        std::fs::create_dir_all(&folder).unwrap();
        let status = serde_json::json!({"models": [
            {"id": "other", "model_path": "/nowhere"},
            {"id": "model-a", "model_path": folder.to_str().unwrap()}
        ]})
        .to_string();
        assert_eq!(
            model_folder(status.as_bytes(), "model-a"),
            Some(folder.clone())
        );
        assert_eq!(model_folder(status.as_bytes(), "missing"), None);

        // Without chat_template.jinja the tokenizer config's template is used: a string, or the
        // `default` of a named list.
        std::fs::write(
            folder.join("tokenizer_config.json"),
            r#"{"chat_template": [{"name": "tool_use", "template": "t"}, {"name": "default", "template": "d"}]}"#,
        )
        .unwrap();
        assert_eq!(read_chat_template(&folder).await.as_deref(), Some("d"));
        std::fs::write(folder.join("chat_template.jinja"), "j").unwrap();
        assert_eq!(read_chat_template(&folder).await.as_deref(), Some("j"));
        assert_eq!(
            template_from_tokenizer_config(br#"{"chat_template": "s"}"#).as_deref(),
            Some("s")
        );
        assert_eq!(read_chat_template(&dir.path().join("none")).await, None);
    }

    /// Reads a live oMLX server's templates and prints the analysis:
    /// `PROCYON_OMLX_URL=http://127.0.0.1:8000 cargo test -p fm-application live_omlx -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "needs a running oMLX server in PROCYON_OMLX_URL"]
    async fn live_omlx_templates() {
        let Ok(base) = std::env::var("PROCYON_OMLX_URL") else {
            return;
        };
        let transport = ReqwestLlmProbeTransport::new();
        let request = |model: &str| LlmProbeRequest {
            url: format!("{base}/v1/chat/completions"),
            ollama_generation_url: None,
            model: model.to_owned(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(10),
            reasoning_switch: ReasoningSwitch::Plain,
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let cancel = CancellationToken::new();
        let models = transport
            .discover_models(&request("x"), &cancel)
            .await
            .unwrap()
            .unwrap_or_default();
        for model in models {
            let template = transport
                .chat_template(&request(&model), &cancel)
                .await
                .unwrap();
            let analysed = template
                .as_deref()
                .and_then(crate::model_thinking::template::analyse);
            println!(
                "{model}: template={} -> {:?}",
                template.is_some(),
                analysed.map(|entry| (
                    entry.thinking,
                    entry.switch,
                    entry.default_on,
                    entry.levels
                ))
            );
        }
    }

    /// How one generation asked to think: its effort and its template switch.
    type AskedThinking = (Option<&'static str>, Option<(&'static str, bool)>);

    /// Serves a chat template for every model and records how each generation asked to think.
    struct TemplateTransport {
        template: Mutex<String>,
        template_reads: std::sync::atomic::AtomicUsize,
        attempts: Mutex<Vec<AskedThinking>>,
    }

    #[async_trait]
    impl LlmProbeTransport for TemplateTransport {
        async fn chat_template(
            &self,
            _request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<Option<String>, LlmProfileError> {
            self.template_reads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Some(self.template.lock().unwrap().clone()))
        }

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
            self.attempts
                .lock()
                .unwrap()
                .push((request.reasoning_effort, request.template_switch));
            Ok(ChatOutcome::Generated {
                text: "fine".into(),
                reasoning_seen: false,
            })
        }
    }

    /// A Dirk-like template: a switch, and efforts none/low/high only (medium raises).
    const DIRK_LIKE: &str = "{%- set e = reasoning_effort|default('high') %}{%- if e not in ('none', 'low', 'high') %}{{- raise_exception('no ' ~ e) }}{%- endif %}{%- if enable_thinking is undefined or enable_thinking is true %}[{{ e }}]<think>{%- else %}<think></think>{%- endif %}";

    #[tokio::test]
    async fn an_omlx_models_template_is_read_once_learned_and_kept_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let transport = Arc::new(TemplateTransport {
            template: Mutex::new(DIRK_LIKE.to_owned()),
            template_reads: std::sync::atomic::AtomicUsize::new(0),
            attempts: Mutex::new(Vec::new()),
        });
        let open = |transport: Arc<TemplateTransport>| {
            LlmProfileService::new(
                SettingsStore::new(dir.path()),
                Arc::new(InMemoryCredentialStore::new()),
                transport,
                LlmHostPolicy::desktop(),
            )
            .unwrap()
        };
        let service = open(transport.clone());
        let mut omlx = draft(LlmPreset::Omlx, "http://127.0.0.1:8000");
        // A Qwen3.8 model: the list would send `medium`, which this template refuses.
        omlx.model = "Dirk-Qwen3.8-27B".into();
        let profile = service.create(omlx).await.unwrap();
        let cancel = CancellationToken::new();
        let ask = |effort| LlmChatGeneration {
            thinking: ThinkingRequest::Conversation(Some(effort)),
            ..tiny_generation()
        };
        service
            .generate(profile.id, ask(ThinkingEffort::High), &cancel)
            .await
            .unwrap();
        service
            .generate(profile.id, ask(ThinkingEffort::Medium), &cancel)
            .await
            .unwrap();
        let reads = |transport: &TemplateTransport| {
            transport
                .template_reads
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        assert_eq!(reads(&transport), 1, "once per session");
        assert_eq!(
            *transport.attempts.lock().unwrap(),
            [
                (Some("high"), Some(("enable_thinking", true))),
                // No medium in this template: the switch alone, never a refused value.
                (None, Some(("enable_thinking", true))),
            ]
        );

        // Settings shows it came from the template, with an entry for the curated list.
        service
            .set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: "Dirk-Qwen3.8-27B".into(),
                    allow_cloud: false,
                    thinking: None,
                }),
            )
            .unwrap();
        let support = service
            .roles_dto()
            .unwrap()
            .chat
            .unwrap()
            .thinking_support
            .unwrap();
        assert_eq!(
            support.origin,
            fm_transport_dto::ThinkingOriginDto::Template
        );
        let learned_at = support.learned_at.clone().unwrap();
        let entry: serde_json::Value = serde_json::from_str(&support.list_entry.unwrap()).unwrap();
        assert_eq!(entry["match"][0], "dirk-qwen3.8-27b");
        assert_eq!(entry["levels"]["high"], "high");

        // A new session reads the template again; unchanged, it keeps what was learned.
        let later = open(transport.clone());
        later
            .generate(profile.id, ask(ThinkingEffort::Low), &cancel)
            .await
            .unwrap();
        assert_eq!(reads(&transport), 2);
        let same = later
            .roles_dto()
            .unwrap()
            .chat
            .unwrap()
            .thinking_support
            .unwrap();
        assert_eq!(same.learned_at.as_deref(), Some(learned_at.as_str()));

        // A changed template is learned anew.
        *transport.template.lock().unwrap() = DIRK_LIKE.replace("'high')", "'low')");
        let after = open(transport.clone());
        after
            .generate(profile.id, ask(ThinkingEffort::Low), &cancel)
            .await
            .unwrap();
        let learned = after
            .learned_thinking
            .get("http://127.0.0.1:8000", "Dirk-Qwen3.8-27B")
            .unwrap();
        assert_eq!(
            learned.template_sha256,
            crate::model_thinking::learned::template_hash(&transport.template.lock().unwrap())
        );
    }

    /// Accepts only the listed `reasoning_effort` values (and none at all), like a strict server.
    struct EffortTransport {
        accepted: Vec<Option<&'static str>>,
        attempts: Mutex<Vec<(Option<&'static str>, Duration)>>,
    }

    #[async_trait]
    impl LlmProbeTransport for EffortTransport {
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
            self.attempts
                .lock()
                .unwrap()
                .push((request.reasoning_effort, request.timeout));
            if self.accepted.contains(&request.reasoning_effort) {
                Ok(ChatOutcome::Generated {
                    text: "fine".into(),
                    reasoning_seen: false,
                })
            } else {
                Ok(ChatOutcome::Rejected)
            }
        }
    }

    #[tokio::test]
    async fn an_effort_the_endpoint_refuses_falls_back_and_the_accepted_one_goes_first() {
        let transport = Arc::new(EffortTransport {
            accepted: vec![Some("minimal"), None],
            attempts: Mutex::new(Vec::new()),
        });
        let service = LlmProfileService::new(
            SettingsStore::new(tempdir().unwrap().keep()),
            Arc::new(InMemoryCredentialStore::new()),
            transport.clone(),
            LlmHostPolicy::desktop(),
        )
        .unwrap();
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        let off = LlmChatGeneration {
            thinking: ThinkingRequest::Conversation(Some(ThinkingEffort::Off)),
            ..tiny_generation()
        };
        let cancel = CancellationToken::new();
        service
            .generate(profile.id, off.clone(), &cancel)
            .await
            .unwrap();
        let tried = |transport: &EffortTransport| -> Vec<Option<&'static str>> {
            transport
                .attempts
                .lock()
                .unwrap()
                .iter()
                .map(|(effort, _)| *effort)
                .collect()
        };
        assert_eq!(tried(&transport), [Some("none"), Some("minimal")]);
        transport.attempts.lock().unwrap().clear();
        service.generate(profile.id, off, &cancel).await.unwrap();
        assert_eq!(tried(&transport), [Some("minimal")]);

        // A level the model does not take ends with a request that asks nothing, and waits as long
        // between chunks as for the first one.
        transport.attempts.lock().unwrap().clear();
        let high = LlmChatGeneration {
            thinking: ThinkingRequest::Conversation(Some(ThinkingEffort::High)),
            ..tiny_generation()
        };
        service.generate(profile.id, high, &cancel).await.unwrap();
        let attempts = transport.attempts.lock().unwrap().clone();
        assert_eq!(
            attempts
                .iter()
                .map(|(effort, _)| *effort)
                .collect::<Vec<_>>(),
            [Some("high"), None]
        );
        assert!(
            attempts
                .iter()
                .all(|(_, timeout)| *timeout >= FIRST_REPLY_MIN_WAIT)
        );

        // A background call asks nothing.
        transport.attempts.lock().unwrap().clear();
        service
            .generate(profile.id, tiny_generation(), &cancel)
            .await
            .unwrap();
        assert_eq!(tried(&transport), [None]);
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
    fn the_loopback_opt_in_opens_the_loopback_branch_and_nothing_else() {
        let policy = LlmHostPolicy::server_with_loopback(["192.168.1.20".to_owned()], true);
        // A model served on the machine itself is now reachable, by name as well as by address.
        for url in [
            "http://127.0.0.1:11234",
            "http://localhost:11434",
            "http://[::1]:8000",
        ] {
            let endpoint = normalize_endpoint(url).unwrap();
            assert!(policy.permits(&endpoint), "{url} should be reachable");
        }
        // The rest of the server posture is untouched: a LAN address still needs listing, and an
        // address outside the list - local-looking or not - still stays out.
        assert!(policy.permits(&normalize_endpoint("http://192.168.1.20:8000").unwrap()));
        assert!(!policy.permits(&normalize_endpoint("http://192.168.1.21:8000").unwrap()));
        assert!(!policy.permits(&normalize_endpoint("https://api.example.test").unwrap()));
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
                    thinking: ThinkingEffort::Auto,
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
                    thinking: None,
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
    async fn the_loopback_opt_in_lets_a_server_reach_a_model_on_its_own_machine() {
        let (service, _, _) = service(LlmHostPolicy::server_with_loopback([], true));
        let local = service
            .create(draft(LlmPreset::Ollama, "http://127.0.0.1:11434"))
            .await
            .unwrap();
        let result = service
            .test(local.id, &CancellationToken::new())
            .await
            .unwrap();
        // Whether anything answers on that port is the network's business now; the gate no longer
        // refuses the request before it leaves, which is what the opt-in is for.
        assert_ne!(result.category, Some(LlmTestErrorCategory::PolicyDenied));
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
            Err(LlmProfileError::ReplyCutOff)
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
            Err(LlmProfileError::ReplyCutOff)
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
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let body = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                followups: Vec::new(),
                tools: Vec::new(),
                system_prompt: "Answer from evidence.".into(),
                user_prompt: "Question".into(),
                images: Vec::new(),
                maximum_tokens: Some(1_024),
                context_window: None,
                temperature: Some(0.2),
                reply: ReplyFormat::Json,
                thinking: crate::llm_profiles::ThinkingRequest::Background,
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
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "Summarize grounded evidence.".into(),
            user_prompt: "Evidence".into(),
            images: Vec::new(),
            maximum_tokens: Some(512),
            context_window: None,
            temperature: Some(0.1),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
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

    #[tokio::test]
    async fn an_endpoint_without_native_tools_reads_the_exchanges_as_text() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        let generation = LlmChatGeneration {
            tools: vec![ToolSpec {
                name: "read_file",
                description: "Reads.",
                schema: json!({"type": "object"}),
            }],
            followups: conversation_with_a_call(),
            ..tiny_generation()
        };
        service
            .generate_streamed(
                profile.id,
                generation,
                &CancellationToken::new(),
                &mut |_| {},
            )
            .await
            .unwrap();
        let prompts = transport.generated_prompts.lock().unwrap();
        let (prompt, followups) = &prompts[0];
        assert_eq!(*followups, 0);
        assert!(prompt.contains("Assistant: Let me look."), "{prompt}");
        assert!(prompt.contains("Tool result:\nalpha"), "{prompt}");
        assert!(transport.generated_tools.lock().unwrap()[0].is_empty());
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
            followups: Vec::new(),
            tools: Vec::new(),
            system_prompt: "Answer.".into(),
            user_prompt: "Question".into(),
            images: Vec::new(),
            maximum_tokens: Some(16),
            context_window: None,
            temperature: Some(0.0),
            reply: ReplyFormat::Text,
            thinking: crate::llm_profiles::ThinkingRequest::Background,
        }
    }

    /// Runs the probe the way the background task does and waits for it.
    async fn probe(service: &LlmProfileService, id: Uuid) -> bool {
        let model = service.profile(id).unwrap().model;
        service.probe_tool_calling(id, &model).await
    }

    #[tokio::test]
    async fn tools_reach_the_endpoint_only_after_the_probe_found_the_model_takes_them() {
        let tool = || ToolSpec {
            name: "read_file",
            description: "reads",
            schema: json!({"type": "object"}),
        };
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        let generate = || async {
            service
                .generate(
                    profile.id,
                    LlmChatGeneration {
                        followups: Vec::new(),
                        tools: vec![tool()],
                        ..tiny_generation()
                    },
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
        };

        // Before any probe answered, tools use the text protocol.
        generate().await;
        // The fake endpoint answers the probe with a tool call.
        transport
            .script_tool_call
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(probe(&service, profile.id).await);
        generate().await;

        let seen = transport.generated_tools.lock().unwrap();
        assert!(
            seen.iter()
                .any(|tools| tools.iter().any(|t| t.name == "echo"))
        );
        assert!(seen[0].is_empty());
        assert_eq!(seen.last().unwrap(), &vec![tool()]);
        assert_eq!(
            service.profile(profile.id).unwrap().tool_calling,
            Some(ToolCallingProbe {
                model: profile.model.clone(),
                supported: true
            })
        );
    }

    #[tokio::test]
    async fn a_model_that_answers_the_probe_in_text_is_recorded_as_not_taking_tools() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();

        assert!(probe(&service, profile.id).await);

        assert_eq!(
            service
                .profile(profile.id)
                .unwrap()
                .tool_calling
                .map(|p| p.supported),
            Some(false)
        );
        assert!(
            !service
                .generation_profile(profile.id)
                .unwrap()
                .takes_native_tools()
        );
        drop(transport);
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_records_nothing() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        transport
            .generate_failures
            .lock()
            .unwrap()
            .push_back(LlmProfileError::Transport);

        assert!(!probe(&service, profile.id).await);
        assert_eq!(service.profile(profile.id).unwrap().tool_calling, None);
    }

    #[tokio::test]
    async fn the_answer_is_kept_until_the_endpoint_or_model_changes() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        transport
            .script_tool_call
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        assert!(probe(&service, profile.id).await);

        let mut same = draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000");
        same.name = "Renamed".into();
        let kept = service.update(profile.id, same).await.unwrap();
        assert!(kept.tool_calling.is_some());

        let other = draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:9000");
        let reset = service.update(profile.id, other).await.unwrap();
        assert_eq!(reset.tool_calling, None);
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

    #[test]
    fn ollama_takes_tools_in_place_of_the_json_constraint_and_returns_a_call_line() {
        let request = LlmProbeRequest {
            url: "http://localhost:11434/v1/chat/completions".into(),
            ollama_generation_url: Some("http://localhost:11434/api/chat".into()),
            model: "m".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Plain,
            reasoning_effort: None,
            ollama_think: OllamaThink::Off,
            template_switch: None,
        };
        let tool = ToolSpec {
            name: "read_file",
            description: "reads",
            schema: json!({"type": "object"}),
        };
        let with_tools = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                followups: Vec::new(),
                tools: vec![tool],
                ..tiny_generation()
            },
        );
        assert!(with_tools.get("format").is_none());
        assert_eq!(with_tools["tools"][0]["function"]["name"], "read_file");
        let json = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                reply: ReplyFormat::Json,
                ..tiny_generation()
            },
        );
        assert_eq!(json["format"], "json");
        assert!(json.get("tools").is_none());
        // Prose (a chat answer, a summary) is never forced into JSON.
        let prose = ollama_generation_body(&request, &tiny_generation());
        assert!(prose.get("format").is_none());

        let line = parse_ollama_chat(
            br#"{"done":true,"message":{"content":"","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"a.txt"}}}]}}"#,
        )
        .unwrap();
        assert_eq!(
            line,
            r#"TOOL {"name":"read_file","arguments":{"path":"a.txt"}}"#
        );
    }

    #[test]
    fn ollama_reports_tool_support_per_model() {
        assert!(
            parse_ollama_model_capabilities(br#"{"capabilities":["completion","tools"]}"#)
                .unwrap()
                .tools
        );
        assert!(
            !parse_ollama_model_capabilities(br#"{"capabilities":["completion"]}"#)
                .unwrap()
                .tools
        );
    }
}
