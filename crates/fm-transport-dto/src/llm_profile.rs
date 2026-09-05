//! Transport types for named OpenAI-compatible generation profiles.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LlmPresetDto {
    Ollama,
    LmStudio,
    Vllm,
    Sglang,
    Omlx,
    OpenAiCompatible,
    AzureOpenAi,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LlmApiCapabilityDto {
    ChatCompletions,
    Responses,
    ModelDiscovery,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LlmEndpointLocalityDto {
    Loopback,
    Cloud,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LlmTlsPolicyDto {
    RequireValidCertificate,
    RequireHttps,
}

/// Conservative generation and transport settings.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmAdvancedSettingsDto {
    pub context_window: u32,
    pub maximum_answer_tokens: u32,
    pub temperature: f32,
    pub timeout_seconds: u64,
    pub tls_policy: LlmTlsPolicyDto,
    pub custom_headers: BTreeMap<String, String>,
}

/// A saved generation profile. Secret material and its opaque store ID are
/// structurally absent.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfileDto {
    pub id: Uuid,
    pub name: String,
    pub preset: LlmPresetDto,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub has_credential: bool,
    pub advanced: LlmAdvancedSettingsDto,
    pub capabilities: BTreeSet<LlmApiCapabilityDto>,
    pub locality: LlmEndpointLocalityDto,
    pub consented_host: Option<String>,
    pub redact_filenames: bool,
}

/// Safe preset defaults returned to profile editors.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfilePresetDto {
    pub name: String,
    pub preset: LlmPresetDto,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub advanced: LlmAdvancedSettingsDto,
    pub capabilities: BTreeSet<LlmApiCapabilityDto>,
    pub redact_filenames: bool,
}

/// Write-only secret input. Its debug representation never includes the key.
#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmCredentialInputDto {
    /// API key or token to place in protected credential storage.
    pub api_key: String,
}

impl fmt::Debug for LlmCredentialInputDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LlmCredentialInputDto")
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// Create/update payload. An omitted credential preserves the saved
/// credential during update and creates a credential-free profile on create.
#[allow(missing_docs)]
#[derive(Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveLlmProfileRequestDto {
    pub name: String,
    pub preset: LlmPresetDto,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub credential: Option<LlmCredentialInputDto>,
    pub advanced: LlmAdvancedSettingsDto,
    pub capabilities: BTreeSet<LlmApiCapabilityDto>,
    pub redact_filenames: bool,
}

/// Explicit treatment of a profile's credential during deletion.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum OrphanLlmCredentialDispositionDto {
    Delete,
    Retain,
}

/// Profile deletion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLlmProfileRequestDto {
    /// Whether to delete or retain the credential after removing the profile.
    pub credential_disposition: OrphanLlmCredentialDispositionDto,
}

/// Activation request carrying explicit informed cloud consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivateLlmProfileRequestDto {
    /// Whether the user consented to sending prompts and retrieved excerpts to
    /// the profile's normalized cloud host.
    pub consent: bool,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LlmTestErrorCategoryDto {
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

/// Content-free result of a bounded synthetic profile test.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfileTestResultDto {
    pub profile_id: Uuid,
    pub provider: LlmPresetDto,
    pub locality: LlmEndpointLocalityDto,
    pub success: bool,
    pub category: Option<LlmTestErrorCategoryDto>,
    pub duration_ms: u64,
    pub model_available: Option<bool>,
    pub available_models: Option<Vec<String>>,
    pub capabilities: BTreeSet<LlmApiCapabilityDto>,
}

/// Exportable non-secret profile configuration.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfileExportDto {
    pub name: String,
    pub preset: LlmPresetDto,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub advanced: LlmAdvancedSettingsDto,
    pub capabilities: BTreeSet<LlmApiCapabilityDto>,
    pub redact_filenames: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_only_credential_debug_does_not_expose_secret() {
        let input = LlmCredentialInputDto {
            api_key: "secret-value".to_owned(),
        };
        assert!(!format!("{input:?}").contains("secret-value"));
    }

    #[test]
    fn profile_response_has_presence_only_and_no_credential_id() {
        let value = serde_json::to_value(LlmProfileDto {
            id: Uuid::nil(),
            name: "Local".to_owned(),
            preset: LlmPresetDto::Ollama,
            base_url: "http://127.0.0.1:11434".to_owned(),
            deployment: None,
            api_version: None,
            model: "model".to_owned(),
            has_credential: true,
            advanced: LlmAdvancedSettingsDto {
                context_window: 8_192,
                maximum_answer_tokens: 1_024,
                temperature: 0.2,
                timeout_seconds: 30,
                tls_policy: LlmTlsPolicyDto::RequireValidCertificate,
                custom_headers: BTreeMap::new(),
            },
            capabilities: BTreeSet::from([LlmApiCapabilityDto::ChatCompletions]),
            locality: LlmEndpointLocalityDto::Loopback,
            consented_host: None,
            redact_filenames: false,
        })
        .unwrap();
        let encoded = value.to_string();
        assert!(encoded.contains("\"hasCredential\":true"));
        assert!(!encoded.to_ascii_lowercase().contains("credentialref"));
        assert!(!encoded.to_ascii_lowercase().contains("apikey"));
    }
}
