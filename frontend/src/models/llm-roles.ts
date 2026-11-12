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

const PRIVATE_IPV4 = [
  /^127\./u,
  /^10\./u,
  /^192\.168\./u,
  /^172\.(1[6-9]|2\d|3[01])\./u,
  /^169\.254\./u,
];

/**
 * Classifies an endpoint as loopback or private LAN, which needs no consent prompt. An unparsable
 * URL counts as cloud so that it is never treated as local by accident.
 */
export function endpointIsLocal(baseUrl: string): boolean {
  let host: string;
  try {
    host = new URL(baseUrl).hostname.replace(/^\[|\]$/gu, '').toLowerCase();
  } catch {
    return false;
  }
  if (host === '::1' || host === 'localhost' || host.endsWith('.localhost')) return true;
  if (host.endsWith('.local')) return true;
  return PRIVATE_IPV4.some((pattern) => pattern.test(host));
}

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
