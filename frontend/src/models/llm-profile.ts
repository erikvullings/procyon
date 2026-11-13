export type {
  ActivateLlmProfileRequestDto as ActivateLlmProfileRequest,
  DeleteLlmProfileRequestDto as DeleteLlmProfileRequest,
  LlmAdvancedSettingsDto as LlmAdvancedSettings,
  LlmApiCapabilityDto as LlmApiCapability,
  LlmEndpointLocalityDto as LlmEndpointLocality,
  LlmModelLimitsDto as LlmModelLimits,
  LlmPresetDto as LlmPreset,
  LlmProfileDto as LlmProfile,
  LlmProfileExportDto as LlmProfileExport,
  LlmProfilePresetDto as LlmProfilePreset,
  LlmProfileTestResultDto as LlmProfileTestResult,
  LlmRoleAssignmentDto as LlmRoleAssignment,
  LlmRolesDto as LlmRoles,
  LlmTestErrorCategoryDto as LlmTestErrorCategory,
  LlmTlsPolicyDto as LlmTlsPolicy,
  OrphanLlmCredentialDispositionDto as OrphanLlmCredentialDisposition,
  SaveLlmProfileRequestDto as SaveLlmProfileRequest,
  SaveLlmRoleRequestDto as SaveLlmRoleRequest,
} from '../api/generated/models';

/** A feature that uses a model: chat, semantic embeddings, or image generation (tasks 0322, 0325). */
export type LlmRole = 'chat' | 'embeddings' | 'image';
