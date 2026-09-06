//! Named OpenAI-compatible generation profiles (task 0184).
//!
//! Profiles are an application capability, not VFS connections. Durable JSON
//! stores only opaque credential references; secret values stay behind
//! [`CredentialStore`] and are resolved only for a bounded request.

#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_credentials::{CredentialRef, CredentialStore, SecretMaterial, StoreCredentialRequest};
use fm_settings::{SettingsStore, VersionedDocument};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};
use uuid::Uuid;

const PROFILE_SCHEMA_VERSION: u32 = 2;
const TEST_PROMPT: &str = "Reply with OK.";
const MAX_STREAM_BYTES: usize = 256 * 1024;
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
    OpenAiCompatible,
    AzureOpenAi,
}

/// Protocol capability advertised by a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmApiCapability {
    ChatCompletions,
    Responses,
    ModelDiscovery,
}

/// Network locality visible wherever a profile is selected.
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
    pub consented_host: Option<String>,
    pub redact_filenames: bool,
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
        match endpoint.locality {
            EndpointLocality::Loopback => self.allow_loopback,
            EndpointLocality::Cloud => {
                self.allow_all_cloud_hosts || self.allowed_cloud_hosts.contains(&endpoint.host)
            }
        }
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
}

#[derive(Debug, Clone)]
pub struct LlmProbeRequest {
    pub url: String,
    pub ollama_generation_url: Option<String>,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_header: &'static str,
    pub headers: BTreeMap<String, String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProbeResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Bounded host-owned generation request using a saved profile.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmChatGeneration {
    /// Trusted system instruction.
    pub system_prompt: String,
    /// User-role evidence payload.
    pub user_prompt: String,
    /// Maximum generated tokens, further bounded by the saved profile.
    pub maximum_tokens: u32,
    /// Sampling temperature, further bounded by the saved profile.
    pub temperature: f32,
}

#[async_trait]
pub trait LlmProbeTransport: Send + Sync {
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
        let mut builder = self.client.request(method, url).timeout(request.timeout);
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
            let response = tokio::select! {
                () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
                response = self.request(reqwest::Method::POST, request, url).json(&body).send() => {
                    response.map_err(map_reqwest)?
                }
            };
            classify_http_status(response.status().as_u16())?;
            let bytes = read_bounded_body(response, cancellation).await?;
            return parse_ollama_chat(&bytes);
        }
        let body = json!({
            "model": request.model,
            "messages": [
                {"role": "system", "content": generation.system_prompt},
                {"role": "user", "content": generation.user_prompt}
            ],
            "stream": true,
            "max_tokens": generation.maximum_tokens,
            "temperature": generation.temperature,
        });
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = self.request(reqwest::Method::POST, request, &request.url).json(&body).send() => {
                response.map_err(map_reqwest)?
            }
        };
        classify_http_status(response.status().as_u16())?;
        let bytes = read_bounded_body(response, cancellation).await?;
        parse_streaming_chat(&bytes)
    }
}

async fn read_bounded_body(
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, LlmProfileError> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let next = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            return Ok(bytes);
        };
        let chunk = chunk.map_err(map_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_STREAM_BYTES {
            return Err(LlmProfileError::MalformedResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
}

/// Durable profile collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmProfilesDocument {
    schema_version: u32,
    profiles: Vec<LlmProfile>,
}

impl Default for LlmProfilesDocument {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: Vec::new(),
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
                value["schemaVersion"] = Value::from(PROFILE_SCHEMA_VERSION);
                if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
                    for profile in profiles {
                        profile["capabilities"] = json!(["chatCompletions", "modelDiscovery"]);
                        profile["redactFilenames"] = Value::Bool(false);
                        profile["consentedHost"] = Value::Null;
                    }
                }
                Ok(value)
            }
            PROFILE_SCHEMA_VERSION => Ok(value),
            _ => Err(LlmProfileError::InvalidConfiguration),
        }
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
        let profiles = settings
            .load_document::<LlmProfilesDocument>()
            .map_err(|_| LlmProfileError::Persistence)?
            .unwrap_or_default();
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
            (LlmPreset::Omlx, "OMLX", "http://127.0.0.1:8080"),
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
        ]
        .into_iter()
        .map(|(preset, name, base_url)| preset_draft(preset, name, base_url))
        .collect()
    }

    pub fn list(&self) -> Result<Vec<LlmProfile>, LlmProfileError> {
        Ok(self.lock()?.profiles.clone())
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
            document.profiles.push(profile.clone());
            match self.persist(&document) {
                Ok(()) => None,
                Err(error) => {
                    document.profiles.pop();
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
        let old_host = normalize_endpoint(&old.base_url)?.host;
        let new_host = normalize_endpoint(&profile.base_url)?.host;
        profile.consented_host = (old_host == new_host)
            .then_some(old.consented_host)
            .flatten();
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
            if let Err(error) = self.persist(&document) {
                document.profiles.insert(index, profile);
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
        let (profile, changed) = {
            let profile = document
                .profiles
                .iter_mut()
                .find(|profile| profile.id == id)
                .ok_or(LlmProfileError::NotFound)?;
            let endpoint = normalize_endpoint(&profile.base_url)?;
            self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
            let mut changed = false;
            if endpoint.locality == EndpointLocality::Cloud
                && profile.consented_host.as_deref() != Some(endpoint.host.as_str())
            {
                if !consent {
                    return Err(LlmProfileError::ConsentRequired(endpoint.host));
                }
                profile.consented_host = Some(endpoint.host);
                changed = true;
            }
            (profile.clone(), changed)
        };
        if changed {
            self.persist(&document)?;
        }
        Ok(profile)
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

    /// Generates bounded text through a saved profile after policy and consent checks.
    pub async fn generate(
        &self,
        id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError> {
        let profile = self.profile(id)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        if endpoint.locality == EndpointLocality::Cloud
            && profile.consented_host.as_deref() != Some(endpoint.host.as_str())
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
        let bounded = LlmChatGeneration {
            maximum_tokens: generation
                .maximum_tokens
                .min(profile.advanced.maximum_answer_tokens),
            temperature: generation
                .temperature
                .clamp(0.0, profile.advanced.temperature),
            ..generation
        };
        self.transport
            .generate_chat(&request, &bounded, cancellation)
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
        self.profile(id)
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
        let api_key = match &profile.credential_ref {
            None => None,
            Some(reference) => {
                let resolved = self
                    .credentials
                    .resolve(reference)
                    .await
                    .map_err(|_| LlmProfileError::Credential)?;
                match resolved.secret {
                    SecretMaterial::Password { password } => {
                        let key = password.to_string();
                        Some(if profile.preset == LlmPreset::AzureOpenAi {
                            key
                        } else {
                            format!("Bearer {key}")
                        })
                    }
                    _ => return Err(LlmProfileError::Credential),
                }
            }
        };
        Ok(LlmProbeRequest {
            url: chat_completions_url(profile, &endpoint)?,
            ollama_generation_url: (profile.preset == LlmPreset::Ollama)
                .then(|| ollama_chat_url(&endpoint)),
            model: profile.model.clone(),
            api_key,
            api_key_header: if profile.preset == LlmPreset::AzureOpenAi {
                "api-key"
            } else {
                "authorization"
            },
            headers: profile.advanced.custom_headers.clone(),
            timeout: Duration::from_secs(profile.advanced.timeout_seconds),
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
    }
}

fn validate_profile(profile: &LlmProfile) -> Result<(), LlmProfileError> {
    if profile.name.trim().is_empty()
        || profile.model.trim().is_empty()
        || profile.advanced.context_window == 0
        || profile.advanced.maximum_answer_tokens == 0
        || profile.advanced.maximum_answer_tokens > profile.advanced.context_window
        || !profile.advanced.temperature.is_finite()
        || !(0.0..=2.0).contains(&profile.advanced.temperature)
        || profile.advanced.timeout_seconds == 0
        || profile.advanced.timeout_seconds > 600
        || !profile
            .capabilities
            .contains(&LlmApiCapability::ChatCompletions)
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
    let locality = match host {
        Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost") => {
            EndpointLocality::Loopback
        }
        Host::Ipv4(address) if address.is_loopback() => EndpointLocality::Loopback,
        Host::Ipv6(address) if address.is_loopback() => EndpointLocality::Loopback,
        _ => EndpointLocality::Cloud,
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
    })
}

pub(crate) fn normalize_endpoint_locality(
    value: &str,
) -> Result<EndpointLocality, LlmProfileError> {
    normalize_endpoint(value).map(|endpoint| endpoint.locality)
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

fn ollama_chat_url(endpoint: &NormalizedEndpoint) -> String {
    let base = endpoint
        .base_url
        .strip_suffix("/v1")
        .unwrap_or(&endpoint.base_url);
    format!("{base}/api/chat")
}

fn ollama_generation_body(request: &LlmProbeRequest, generation: &LlmChatGeneration) -> Value {
    json!({
        "model": request.model,
        "messages": [
            {"role": "system", "content": generation.system_prompt},
            {"role": "user", "content": generation.user_prompt}
        ],
        "stream": false,
        "think": false,
        "options": {
            "num_predict": generation.maximum_tokens,
            "temperature": generation.temperature
        }
    })
}

fn validate_streaming_chat(bytes: &[u8]) -> Result<(), LlmProfileError> {
    parse_streaming_chat(bytes).map(|_| ())
}

fn parse_streaming_chat(bytes: &[u8]) -> Result<String, LlmProfileError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LlmProfileError::MalformedResponse)?;
    let mut valid_event = false;
    let mut terminated = false;
    let mut truncated = false;
    let mut content = String::new();
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
        Ok(content)
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
#[derive(Debug, Error)]
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
            Self::PolicyDenied => LlmTestErrorCategory::PolicyDenied,
            Self::ConsentRequired(_) => LlmTestErrorCategory::ConsentRequired,
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

#[cfg(test)]
mod tests {
    use super::*;
    use fm_credentials::InMemoryCredentialStore;
    use tempfile::tempdir;

    struct FakeTransport {
        models: Option<Vec<String>>,
        response: LlmProbeResponse,
        captured: Mutex<Vec<LlmProbeRequest>>,
    }

    #[async_trait]
    impl LlmProbeTransport for FakeTransport {
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
        assert_eq!(presets.len(), 7);
        assert_eq!(presets[0].base_url, "http://127.0.0.1:11434");
        assert_eq!(presets[1].base_url, "http://127.0.0.1:1234");
        assert_eq!(presets[2].base_url, "http://127.0.0.1:8000");
        assert_eq!(presets[3].base_url, "http://127.0.0.1:30000");
        assert_eq!(presets[4].base_url, "http://127.0.0.1:8080");
        assert!(presets[5].redact_filenames);
        assert_eq!(presets[6].api_version.as_deref(), Some("2024-10-21"));
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
            profiles: vec![profile],
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
        assert_eq!(document.profiles[0].consented_host, None);
        assert!(
            document.profiles[0]
                .capabilities
                .contains(&LlmApiCapability::ChatCompletions)
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
            Some("one.example")
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
        };
        let body = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                system_prompt: "Answer from evidence.".into(),
                user_prompt: "Question".into(),
                maximum_tokens: 1_024,
                temperature: 0.2,
            },
        );
        assert_eq!(body["think"], false);
        assert_eq!(body["stream"], false);
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
            maximum_tokens: 512,
            temperature: 0.1,
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
}
