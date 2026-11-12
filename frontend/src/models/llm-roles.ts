import type { LlmProfile, LlmRole, LlmRoleAssignment } from './llm-profile';

/** Roles with a consumer today; image generation and natural-language search are reserved (0322). */
export const LLM_ROLES: readonly LlmRole[] = ['chat', 'embeddings'];

/** Why a role cannot currently use its endpoint, or `ready` when it can. */
export type RoleStatus =
  | 'unassigned'
  | 'missingProfile'
  | 'cloudNotAllowed'
  | 'needsConsent'
  | 'ready';

/** Whether a profile may be assigned to a role, before consent is considered. */
export function canAssignRole(profile: LlmProfile, role: LlmRole, allowCloud: boolean): boolean {
  if (profile.locality !== 'cloud') return true;
  return role !== 'embeddings' || allowCloud;
}

/** The state of a role binding, computed from the saved profiles. The server re-checks it. */
export function roleStatus(
  assignment: LlmRoleAssignment | null | undefined,
  role: LlmRole,
  profiles: readonly LlmProfile[],
): RoleStatus {
  if (assignment == null) return 'unassigned';
  const profile = profiles.find((candidate) => candidate.id === assignment.profileId);
  if (profile === undefined) return 'missingProfile';
  if (profile.locality === 'cloud') {
    if (role === 'embeddings' && !assignment.allowCloud) return 'cloudNotAllowed';
    if (profile.consentedHost == null) return 'needsConsent';
  }
  return 'ready';
}

/**
 * The profile a chat-style feature should start with: the chat role's endpoint when it still exists,
 * otherwise the first saved profile.
 */
export function preferredProfile(
  profiles: readonly LlmProfile[],
  chat: LlmRoleAssignment | null | undefined,
): LlmProfile | undefined {
  return profiles.find((profile) => profile.id === chat?.profileId) ?? profiles[0];
}
