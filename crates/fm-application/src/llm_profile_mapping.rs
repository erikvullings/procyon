//! Explicit mapping between generation-profile domain values and host DTOs.

use crate::llm_profiles::{
    EndpointLocality, LlmAdvancedSettings, LlmApiCapability, LlmPreset, LlmProfile,
    LlmProfileDraft, LlmProfileError, LlmProfileTestResult, LlmTestErrorCategory, LlmTlsPolicy,
    OrphanCredentialDisposition, normalize_endpoint_locality,
};
use fm_transport_dto::{
    LlmAdvancedSettingsDto, LlmApiCapabilityDto, LlmEndpointLocalityDto, LlmPresetDto,
    LlmProfileDto, LlmProfileExportDto, LlmProfilePresetDto, LlmProfileTestResultDto,
    LlmTestErrorCategoryDto, LlmTlsPolicyDto, OrphanLlmCredentialDispositionDto,
    SaveLlmProfileRequestDto,
};

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
        LlmPresetDto::OpenAiCompatible => LlmPreset::OpenAiCompatible,
        LlmPresetDto::AzureOpenAi => LlmPreset::AzureOpenAi,
    }
}

fn preset_to_dto(value: LlmPreset) -> LlmPresetDto {
    match value {
        LlmPreset::Ollama => LlmPresetDto::Ollama,
        LlmPreset::LmStudio => LlmPresetDto::LmStudio,
        LlmPreset::Vllm => LlmPresetDto::Vllm,
        LlmPreset::Sglang => LlmPresetDto::Sglang,
        LlmPreset::Omlx => LlmPresetDto::Omlx,
        LlmPreset::OpenAiCompatible => LlmPresetDto::OpenAiCompatible,
        LlmPreset::AzureOpenAi => LlmPresetDto::AzureOpenAi,
    }
}

fn capability_from_dto(value: LlmApiCapabilityDto) -> LlmApiCapability {
    match value {
        LlmApiCapabilityDto::ChatCompletions => LlmApiCapability::ChatCompletions,
        LlmApiCapabilityDto::Responses => LlmApiCapability::Responses,
        LlmApiCapabilityDto::ModelDiscovery => LlmApiCapability::ModelDiscovery,
    }
}

fn capability_to_dto(value: LlmApiCapability) -> LlmApiCapabilityDto {
    match value {
        LlmApiCapability::ChatCompletions => LlmApiCapabilityDto::ChatCompletions,
        LlmApiCapability::Responses => LlmApiCapabilityDto::Responses,
        LlmApiCapability::ModelDiscovery => LlmApiCapabilityDto::ModelDiscovery,
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
