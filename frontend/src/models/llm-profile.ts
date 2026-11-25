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
  ThinkingEffortDto as ThinkingEffort,
} from '../api/generated/models';

import type { ThinkingEffortDto } from '../api/generated/models';

/** Thinking efforts in order (task 0361); `auto` is the endpoint's usual behaviour. */
export const THINKING_EFFORTS: readonly ThinkingEffortDto[] = [
  'auto',
  'off',
  'low',
  'medium',
  'high',
];

/** Whether `value` names a thinking effort. */
export function isThinkingEffort(value: string): value is ThinkingEffortDto {
  return (THINKING_EFFORTS as readonly string[]).includes(value);
}

/** A feature that uses a model: chat, semantic embeddings, or image generation (tasks 0322, 0325). */
export type LlmRole = 'chat' | 'embeddings' | 'image';
