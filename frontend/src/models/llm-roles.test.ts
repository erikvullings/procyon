import { describe, expect, it } from 'vitest';
import type { LlmProfile } from './llm-profile';
import { canAssignRole, preferredProfile, type RoleStatus, roleStatus } from './llm-roles';

function profile(overrides: Partial<LlmProfile> & Pick<LlmProfile, 'id'>): LlmProfile {
  return {
    name: 'Profile',
    preset: 'ollama',
    baseUrl: 'http://127.0.0.1:11434',
    deployment: null,
    apiVersion: null,
    model: 'llama',
    hasCredential: false,
    advanced: {
      contextWindow: 8192,
      maximumAnswerTokens: 1024,
      temperature: 0.2,
      timeoutSeconds: 30,
      tlsPolicy: 'requireValidCertificate',
      customHeaders: {},
    },
    capabilities: ['chatCompletions'],
    locality: 'loopback',
    consentedHost: null,
    redactFilenames: false,
    ...overrides,
  } as LlmProfile;
}

const cloud = profile({
  id: 'cloud',
  baseUrl: 'https://api.example.test',
  locality: 'cloud',
});

describe('canAssignRole', () => {
  it('lets any local endpoint serve either role', () => {
    const local = profile({ id: 'local' });
    expect(canAssignRole(local, 'chat', false)).toBe(true);
    expect(canAssignRole(local, 'embeddings', false)).toBe(true);
  });

  it('refuses a cloud endpoint for embeddings until cloud use is allowed', () => {
    expect(canAssignRole(cloud, 'embeddings', false)).toBe(false);
    expect(canAssignRole(cloud, 'embeddings', true)).toBe(true);
    expect(canAssignRole(cloud, 'chat', false)).toBe(true);
  });
});

describe('roleStatus', () => {
  const profiles = [profile({ id: 'local' }), cloud];
  const assignment = (profileId: string, allowCloud = false) => ({
    profileId,
    model: 'm',
    allowCloud,
  });

  it.each<[string, Parameters<typeof roleStatus>[0], RoleStatus]>([
    ['unassigned', null, 'unassigned'],
    ['missing profile', assignment('gone'), 'missingProfile'],
    ['local ready', assignment('local'), 'ready'],
    ['cloud chat without consent', assignment('cloud'), 'needsConsent'],
    ['cloud embeddings without flag', assignment('cloud'), 'cloudNotAllowed'],
  ])('reports %s', (_name, value, expected) => {
    const role = expected === 'cloudNotAllowed' ? 'embeddings' : 'chat';
    expect(roleStatus(value, role, profiles)).toBe(expected);
  });

  it('is ready for a consented cloud endpoint with the flag set', () => {
    const consented = { ...cloud, consentedHost: 'https://api.example.test' };
    expect(
      roleStatus(assignment('cloud', true), 'embeddings', [profile({ id: 'local' }), consented]),
    ).toBe('ready');
  });
});

describe('preferredProfile', () => {
  const first = profile({ id: 'first' });
  const second = profile({ id: 'second' });
  const chat = (profileId: string) => ({ profileId, model: 'm', allowCloud: false });

  it('prefers the profile bound to the chat role', () => {
    expect(preferredProfile([first, second], chat('second'))?.id).toBe('second');
  });

  it('falls back to the first saved profile when the role is unassigned or stale', () => {
    expect(preferredProfile([first, second], null)?.id).toBe('first');
    expect(preferredProfile([first, second], chat('gone'))?.id).toBe('first');
  });

  it('returns undefined when there are no profiles', () => {
    expect(preferredProfile([], chat('x'))).toBeUndefined();
  });
});
