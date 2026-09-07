import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import { LlmProfileManagement } from './llm-profile-management';

let root: HTMLElement;

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

async function mountLoaded(
  client: MockFileManagerClient,
  onSaveHandlerChange?: (handler: (() => Promise<boolean>) | undefined) => void,
): Promise<void> {
  const attrs = onSaveHandlerChange === undefined ? { client } : { client, onSaveHandlerChange };
  m.mount(root, {
    view: () => m(LlmProfileManagement, attrs),
  });
  await vi.waitFor(() => expect(root.textContent).not.toContain('Loading generation profiles'));
  m.redraw.sync();
}

function fieldInput(label: string): HTMLInputElement | undefined {
  return (
    [...root.querySelectorAll('.input-field')]
      .find((field) => field.querySelector('label')?.textContent === label)
      ?.querySelector('input') ?? undefined
  );
}

describe('LlmProfileManagement', () => {
  it('loads provider defaults and exposes an accessible profile form', async () => {
    const client = new MockFileManagerClient();
    await mountLoaded(client);

    expect(root.querySelector('[aria-label="Generation profiles"]')).not.toBeNull();
    expect(
      [...root.querySelectorAll<HTMLInputElement>('input')].some(
        (input) => input.value === 'Ollama',
      ),
    ).toBe(true);
    expect(root.textContent).toContain('Provider preset');
    expect(root.textContent).toContain('Base URL');
    expect(root.textContent).toContain('API key or token');
  });

  it('does not repeat the advanced disclosure label inside a fieldset', async () => {
    const client = new MockFileManagerClient();
    await mountLoaded(client);

    const advanced = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'Advanced settings',
    );
    advanced?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-llm-advanced-settings')).not.toBeNull();
    expect(
      [...root.querySelectorAll('legend')].some(
        (legend) => legend.textContent?.trim() === 'Advanced settings',
      ),
    ).toBe(false);
  });

  it('keeps local/cloud and informed-consent status visible for saved profiles', async () => {
    const client = new MockFileManagerClient();
    const profile = await client.createLlmProfile({
      name: 'Private cloud',
      preset: 'openAiCompatible',
      baseUrl: 'https://llm.example.test',
      deployment: null,
      apiVersion: null,
      model: 'model-a',
      credential: { apiKey: 'never-render-this' },
      advanced: {
        contextWindow: 8192,
        maximumAnswerTokens: 1024,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireHttps',
        customHeaders: {},
      },
      capabilities: ['chatCompletions'],
      redactFilenames: true,
    });
    await mountLoaded(client);

    const profileButton = [...root.querySelectorAll<HTMLButtonElement>('button')].find((button) =>
      button.textContent?.includes('Private cloud'),
    );
    expect(profileButton).toBeDefined();
    profileButton?.click();
    m.redraw.sync();

    expect(root.textContent).toContain('Cloud');
    expect(root.textContent).toContain('llm.example.test');
    expect(root.textContent).toContain('questions, conversation history, and retrieved excerpts');
    expect(root.textContent).not.toContain('never-render-this');
    expect(profile.hasCredential).toBe(true);
  });

  it('integrates models discovered by a saved provider into the model field', async () => {
    const client = new MockFileManagerClient();
    const profile = await client.createLlmProfile({
      name: 'Local Ollama',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'model-a',
      credential: null,
      advanced: {
        contextWindow: 8192,
        maximumAnswerTokens: 1024,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireValidCertificate',
        customHeaders: {},
      },
      capabilities: ['chatCompletions', 'modelDiscovery'],
      redactFilenames: false,
    });
    const testProfile = vi.spyOn(client, 'testLlmProfile').mockResolvedValue({
      profileId: profile.id,
      provider: 'ollama',
      locality: 'loopback',
      success: true,
      category: null,
      durationMs: 1,
      modelAvailable: true,
      availableModels: ['model-a', 'model-b'],
      capabilities: ['chatCompletions', 'modelDiscovery'],
    });
    await mountLoaded(client);

    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('Local Ollama'))
      ?.click();
    m.redraw.sync();
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Test')
      ?.click();

    await vi.waitFor(() => {
      expect(fieldInput('Model')?.value).toBe('model-a');
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true);
    });
    expect(root.textContent).not.toContain('Available provider models');
    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll<HTMLButtonElement>('button')].find(
          (button) => button.textContent?.trim() === 'Test',
        )?.disabled,
      ).toBe(false),
    );

    testProfile.mockRejectedValueOnce(new Error('Provider unavailable'));
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Test')
      ?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Provider unavailable'));
    expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(false);
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Test')
      ?.click();
    await vi.waitFor(() =>
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true),
    );

    const discovered = fieldInput('Model');
    discovered?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')]
      .find((item) => item.textContent?.trim() === 'model-b')
      ?.click();
    m.redraw.sync();

    expect(fieldInput('Model')?.value).toBe('model-b');
    expect(
      [...root.querySelectorAll('.input-field')].filter(
        (field) => field.querySelector('label')?.textContent === 'Model',
      ),
    ).toHaveLength(1);
    expect(
      [...root.querySelectorAll<HTMLButtonElement>('button')].find(
        (button) => button.textContent?.trim() === 'Test',
      )?.disabled,
    ).toBe(true);
  });

  it('discovers local provider models immediately after saving a profile', async () => {
    const client = new MockFileManagerClient();
    const discover = vi
      .spyOn(client, 'discoverLlmProfileModels')
      .mockResolvedValue(['llama3.2:3b', 'qwen3:14b']);
    let save: (() => Promise<boolean>) | undefined;
    await mountLoaded(client, (handler) => {
      save = handler;
    });

    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'New profile')
      ?.click();
    m.redraw.sync();
    if (save === undefined) throw new Error('Save handler was not registered');
    await save();

    await vi.waitFor(() => expect(discover).toHaveBeenCalledOnce());
    const discoveredModels = await vi.waitFor(() => {
      const input = fieldInput('Model');
      expect(input?.classList.contains('select-dropdown')).toBe(true);
      return input;
    });
    discoveredModels?.click();
    m.redraw.sync();
    expect(root.textContent).toContain('qwen3:14b');
  });

  it('discovers models for a new Ollama profile before it can be saved', async () => {
    const client = new MockFileManagerClient();
    const discover = vi
      .spyOn(client, 'discoverLlmProfileDraftModels')
      .mockResolvedValue(['llama3.2:3b', 'qwen3:14b']);

    await mountLoaded(client);

    await vi.waitFor(() => expect(discover).toHaveBeenCalledOnce());
    await vi.waitFor(() => {
      expect(fieldInput('Model')?.value).toBe('llama3.2:3b');
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true);
    });
    expect(root.textContent).not.toContain('Available provider models');
  });

  it('selects and discovers the first saved local profile on load', async () => {
    const client = new MockFileManagerClient();
    const profile = await client.createLlmProfile({
      name: 'Local Ollama',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'llama3.2:3b',
      credential: null,
      advanced: {
        contextWindow: 8192,
        maximumAnswerTokens: 1024,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireValidCertificate',
        customHeaders: {},
      },
      capabilities: ['chatCompletions', 'modelDiscovery'],
      redactFilenames: false,
    });
    const discover = vi
      .spyOn(client, 'discoverLlmProfileModels')
      .mockResolvedValue(['llama3.2:3b', 'qwen3:14b']);

    await mountLoaded(client);

    await vi.waitFor(() => expect(discover).toHaveBeenCalledWith(profile.id));
    await vi.waitFor(() =>
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true),
    );
    expect(
      [...root.querySelectorAll<HTMLButtonElement>('button')]
        .find((button) => button.textContent?.includes('Local Ollama'))
        ?.hasAttribute('aria-pressed'),
    ).toBe(true);
  });

  it('surfaces structured host errors instead of replacing their message', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'discoverLlmProfileDraftModels').mockRejectedValue({
      code: 'invalidConfiguration',
      message: 'LLM profile failed validation',
      requestId: 'request-1',
      details: null,
    });

    await mountLoaded(client);

    await vi.waitFor(() => expect(root.textContent).toContain('LLM profile failed validation'));
    expect(root.textContent).not.toContain('Unknown generation-profile error');
  });
});
