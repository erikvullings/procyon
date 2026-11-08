import { describe, expect, it, vi } from 'vitest';
import type { LlmProfile } from '../../models';
import {
  blocksGeneration,
  checkProfileModel,
  modelIsListed,
  profileModelNotice,
} from './profile-model-check';

function profile(overrides: Partial<LlmProfile> = {}): LlmProfile {
  return {
    id: 'p1',
    name: 'Ollama',
    preset: 'ollama',
    model: 'qwen3:14b',
    capabilities: ['chatCompletions', 'modelDiscovery'],
    ...overrides,
  } as LlmProfile;
}

describe('profile model check', () => {
  it('matches exact names and the implicit :latest tag', () => {
    expect(modelIsListed('qwen3:14b', ['llama3.2:3b', 'qwen3:14b'])).toBe(true);
    expect(modelIsListed('llama3.2', ['llama3.2:latest'])).toBe(true);
    expect(modelIsListed('ornith-1.5:9b', ['llama3.2:3b'])).toBe(false);
    expect(modelIsListed('', ['a'])).toBe(false);
  });

  it('reports a configured model the service does not offer as missing', async () => {
    const client = { discoverLlmProfileModels: vi.fn().mockResolvedValue(['llama3.2:3b']) };
    const status = await checkProfileModel(client, profile({ model: 'ornith-1.5:9b' }));
    expect(status).toBe('missing');
    expect(profileModelNotice(profile({ model: 'ornith-1.5:9b' }), status)).toContain(
      'ornith-1.5:9b',
    );
  });

  it('treats an installed model as available and a failing service as unreachable', async () => {
    expect(
      await checkProfileModel(
        { discoverLlmProfileModels: vi.fn().mockResolvedValue(['qwen3:14b']) },
        profile(),
      ),
    ).toBe('available');
    const status = await checkProfileModel(
      { discoverLlmProfileModels: vi.fn().mockRejectedValue(new Error('down')) },
      profile(),
    );
    expect(status).toBe('unreachable');
    expect(profileModelNotice(profile(), status)).toContain('Ollama');
  });

  it('never calls a service that cannot list models, and warns about nothing then', async () => {
    const discover = vi.fn();
    const status = await checkProfileModel(
      { discoverLlmProfileModels: discover },
      profile({ capabilities: ['chatCompletions'] }),
    );
    expect(status).toBe('unverified');
    expect(discover).not.toHaveBeenCalled();
    expect(profileModelNotice(profile(), status)).toBeUndefined();
    expect(
      await checkProfileModel(
        { discoverLlmProfileModels: vi.fn().mockResolvedValue([]) },
        profile(),
      ),
    ).toBe('unverified');
  });

  it('reports a service that wants an API key and blocks generation for it', async () => {
    const rejection = {
      code: 'platformOperationFailed',
      message: 'x',
      details: { reason: 'llmAuthentication' },
    };
    const status = await checkProfileModel(
      { discoverLlmProfileModels: vi.fn().mockRejectedValue(rejection) },
      profile({ name: 'OMLX', preset: 'omlx' }),
    );
    expect(status).toBe('authRequired');
    expect(profileModelNotice(profile({ name: 'OMLX' }), status)).toContain(
      'requires a valid API key',
    );
    expect(blocksGeneration(status)).toBe(true);
    expect(blocksGeneration('missing')).toBe(true);
    expect(blocksGeneration('unreachable')).toBe(false);
    expect(blocksGeneration(undefined)).toBe(false);
  });
});
