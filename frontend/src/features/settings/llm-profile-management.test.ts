import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import { imagesApiOf, LlmProfileManagement, withImagesApi } from './llm-profile-management';

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

function fieldInput(label: string): HTMLInputElement | HTMLButtonElement | undefined {
  return (
    [...root.querySelectorAll('.input-field')]
      .find((field) => field.querySelector('label')?.textContent === label)
      ?.querySelector<HTMLInputElement | HTMLButtonElement>('input, button.select-dropdown') ??
    undefined
  );
}

function fieldValue(label: string): string | undefined {
  const control = fieldInput(label);
  return control instanceof HTMLButtonElement ? (control.textContent ?? undefined) : control?.value;
}

describe('the images API choice', () => {
  it('opens filled from the provider chosen in the roles section', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, {
      view: () =>
        m(LlmProfileManagement, { client, openPreset: { presetId: 'mlxServe', revision: 1 } }),
    });
    await vi.waitFor(() => expect(root.textContent).not.toContain('Loading generation profiles'));
    m.redraw.sync();

    // The picture model is named in the roles section, so its provider arrives from there (0325).
    await vi.waitFor(() => expect(fieldInput('Base URL')?.value).toBe('http://127.0.0.1:11234'));
    expect(fieldInput('Profile name')?.value).toBe('mlx-serve');

    // The same request must not be applied twice: what the person typed stays put.
    const name = fieldInput('Profile name');
    if (name !== undefined) {
      name.value = 'Studio box';
      name.dispatchEvent(new InputEvent('input', { bubbles: true }));
    }
    m.redraw.sync();
    expect(fieldInput('Profile name')?.value).toBe('Studio box');
    expect(fieldInput('Base URL')?.value).toBe('http://127.0.0.1:11234');
  });

  it('records exactly one contract', () => {
    expect(withImagesApi(['chatCompletions', 'modelDiscovery'], 'dashscopeTasks')).toEqual([
      'chatCompletions',
      'modelDiscovery',
      'dashscopeTasks',
    ]);
    expect(withImagesApi(['chatCompletions', 'dashscopeTasks'], 'imageGenerations')).toEqual([
      'chatCompletions',
      'imageGenerations',
    ]);
  });

  it('defaults an endpoint saved before the choice existed to the OpenAI-style API', () => {
    expect(imagesApiOf(['chatCompletions', 'modelDiscovery'])).toBe('imageGenerations');
    expect(imagesApiOf([])).toBe('imageGenerations');
    expect(imagesApiOf(['dashscopeTasks'])).toBe('dashscopeTasks');
  });
});

describe('LlmProfileManagement', () => {
  it('asks which images contract the endpoint answers, in the advanced settings', async () => {
    const client = new MockFileManagerClient();
    await mountLoaded(client);
    const toggle = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent === 'Advanced settings',
    );
    expect(toggle, 'the editor offers advanced settings').toBeDefined();
    toggle?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-llm-advanced-settings')).not.toBeNull());
    m.redraw.sync();

    const advanced = root.querySelector('.fm-llm-advanced-settings');
    expect(
      advanced?.querySelector('[aria-label="Images API"]'),
      'a choice of images API',
    ).toBeDefined();
    // The choice only matters for the image role, and the editor says so rather than leaving a
    // second API selector unexplained next to the chat ones.
    expect(advanced?.textContent).toContain(
      'Used when this endpoint is assigned to the Image role',
    );
  });

  it('loads provider defaults and exposes an accessible profile form', async () => {
    const client = new MockFileManagerClient();
    await mountLoaded(client);

    expect(await client.listLlmProfilePresets()).toContainEqual(
      expect.objectContaining({
        name: 'mlx-serve',
        preset: 'mlxServe',
        baseUrl: 'http://127.0.0.1:11234',
        capabilities: ['chatCompletions', 'modelDiscovery'],
      }),
    );
    // The image role is one choice away: a preset that already names the Qwen-Image model.
    expect(await client.listLlmProfilePresets()).toContainEqual(
      expect.objectContaining({
        name: 'Qwen Image (Alibaba Cloud)',
        preset: 'qwenImage',
        baseUrl: 'https://dashscope.aliyuncs.com/api/v1',
        model: 'qwen-image',
        capabilities: ['dashscopeTasks'],
        redactFilenames: true,
      }),
    );
    expect(root.querySelector('[aria-label="AI summaries and answers"]')).not.toBeNull();
    expect(root.textContent).toContain('No generation profiles configured.');
    expect(await client.listLlmProfiles()).toEqual([]);
    expect(
      [...root.querySelectorAll<HTMLInputElement>('input')].some(
        (input) => input.value === 'Ollama',
      ),
    ).toBe(true);
    expect(root.textContent).toContain('Provider preset');
    expect(root.textContent).toContain('Base URL');
    expect(root.textContent).toContain('API key or token');
    expect(root.textContent).toContain('write document summaries and answer questions');
    expect(root.textContent).toContain('Semantic search works without a profile');
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
    await vi.waitFor(() => expect(fieldValue('Context window')).toBe('128k'));
    expect(fieldValue('Maximum answer tokens')).toBe('8k');
    expect(root.textContent).toContain('131,072 context tokens, 8,192 output tokens');
    fieldInput('Context window')?.click();
    m.redraw.sync();
    expect(
      [...root.querySelectorAll('li')].some((item) => item.textContent?.trim() === '128k'),
    ).toBe(true);
    expect(
      [...root.querySelectorAll('li')].some((item) => item.textContent?.trim() === '256k'),
    ).toBe(false);
    expect(root.textContent).toContain('Maximum input and output combined');
    expect(root.textContent).toContain('This also counts toward the context window');
    expect(root.textContent).not.toContain('Unlimited');
  });

  it('keeps conservative starting caps when model limits are unavailable', async () => {
    const client = new MockFileManagerClient();
    const discover = vi.spyOn(client, 'discoverLlmProfileDraftModelLimits').mockResolvedValue({
      contextWindow: null,
      maximumAnswerTokens: null,
      contextWindowCap: 1_048_576,
      maximumAnswerTokensCap: 16_384,
    });
    await mountLoaded(client);
    await vi.waitFor(() => expect(discover).toHaveBeenCalledOnce());
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    m.redraw.sync();

    expect(fieldValue('Context window')).toBe('8k');
    expect(fieldValue('Maximum answer tokens')).toBe('1k');
    expect(root.textContent).toContain('Model limits could not be determined');
  });

  it('offers the backend-capped 1M context and no output above 16k', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'discoverLlmProfileDraftModelLimits').mockResolvedValue({
      contextWindow: 1_048_576,
      maximumAnswerTokens: null,
      contextWindowCap: 1_048_576,
      maximumAnswerTokensCap: 16_384,
    });
    await mountLoaded(client);
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    await vi.waitFor(() => expect(fieldValue('Context window')).toBe('1M'));
    fieldInput('Maximum answer tokens')?.click();
    m.redraw.sync();
    const options = [...root.querySelectorAll('li')].map((item) => item.textContent?.trim());
    expect(options).toContain('16k');
    expect(options).not.toContain('32k');
    expect(options).not.toContain('Unlimited');
  });

  it('resets automatically suggested limits on model change without overriding edited caps', async () => {
    const client = new MockFileManagerClient();
    await mountLoaded(client);
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    await vi.waitFor(() => expect(fieldValue('Context window')).toBe('128k'));

    fieldInput('Model')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')]
      .find((item) => item.textContent?.trim() === 'model-b')
      ?.click();
    await vi.waitFor(() => expect(fieldValue('Context window')).toBe('8k'));
    expect(fieldValue('Maximum answer tokens')).toBe('1k');

    fieldInput('Context window')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')].find((item) => item.textContent?.trim() === '16k')?.click();
    m.redraw.sync();
    fieldInput('Model')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')]
      .find((item) => item.textContent?.trim() === 'model-a')
      ?.click();
    await vi.waitFor(() => expect(fieldValue('Maximum answer tokens')).toBe('8k'));
    expect(fieldValue('Context window')).toBe('16k');
  });

  it('keeps existing non-preset token limits selectable without modifying a saved profile', async () => {
    const client = new MockFileManagerClient();
    const existing = await client.createLlmProfile({
      name: 'Existing',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'model-a',
      credential: null,
      advanced: {
        contextWindow: 12_000,
        maximumAnswerTokens: 3_000,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireValidCertificate',
        customHeaders: {},
      },
      capabilities: ['chatCompletions', 'modelDiscovery'],
      redactFilenames: false,
    });
    await mountLoaded(client);
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    m.redraw.sync();

    expect(fieldValue('Context window')).toBe('Current: 12,000 tokens');
    expect(fieldValue('Maximum answer tokens')).toBe('Current: 3,000 tokens');
    expect((await client.listLlmProfiles())[0]?.advanced).toEqual(existing.advanced);
  });

  it('warns without rewriting a saved limit above the model report', async () => {
    const client = new MockFileManagerClient();
    const existing = await client.createLlmProfile({
      name: 'Large existing',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'model-a',
      credential: null,
      advanced: {
        contextWindow: 262_144,
        maximumAnswerTokens: 12_288,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireValidCertificate',
        customHeaders: {},
      },
      capabilities: ['chatCompletions', 'modelDiscovery'],
      redactFilenames: false,
    });
    await mountLoaded(client);
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('above a reported model limit'));
    expect(fieldValue('Context window')).toBe('Current: 262,144 tokens');
    expect(fieldValue('Maximum answer tokens')).toBe('Current: 12,288 tokens');
    expect((await client.listLlmProfiles())[0]?.advanced).toEqual(existing.advanced);
  });

  it('saves independently selected context and answer caps', async () => {
    const client = new MockFileManagerClient();
    let save: (() => Promise<boolean>) | undefined;
    await mountLoaded(client, (handler) => {
      save = handler;
    });
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Advanced settings')
      ?.click();
    m.redraw.sync();

    fieldInput('Context window')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')].find((item) => item.textContent?.trim() === '16k')?.click();
    m.redraw.sync();
    fieldInput('Maximum answer tokens')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('li')].find((item) => item.textContent?.trim() === '4k')?.click();
    m.redraw.sync();

    expect(fieldValue('Context window')).toBe('16k');
    expect(fieldValue('Maximum answer tokens')).toBe('4k');
    if (save === undefined) throw new Error('Save handler was not registered');
    expect(await save()).toBe(true);
    expect((await client.listLlmProfiles())[0]?.advanced.contextWindow).toBe(16_384);
    expect((await client.listLlmProfiles())[0]?.advanced.maximumAnswerTokens).toBe(4_096);
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
    expect(root.textContent).toContain('https://llm.example.test');
    expect(root.textContent).toContain('documents you choose to summarize');
    expect(root.textContent).not.toContain('never-render-this');
    expect(profile.hasCredential).toBe(true);
  });

  it('retries a failed consent grant by updating the saved profile, not creating another', async () => {
    const client = new MockFileManagerClient();
    const createProfile = vi.spyOn(client, 'createLlmProfile');
    const updateProfile = vi.spyOn(client, 'updateLlmProfile');
    const grant = vi
      .spyOn(client, 'grantLlmProfileConsent')
      .mockRejectedValueOnce(new Error('consent store unavailable'));
    let save: (() => Promise<boolean>) | undefined;
    await mountLoaded(client, (handler) => {
      save = handler;
    });

    const nameInput = fieldInput('Profile name');
    const urlInput = fieldInput('Base URL');
    if (nameInput === undefined || urlInput === undefined) throw new Error('fields not rendered');
    nameInput.value = 'Cloud';
    nameInput.dispatchEvent(new Event('input', { bubbles: true }));
    urlInput.value = 'https://api.example.test';
    urlInput.dispatchEvent(new Event('input', { bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLInputElement>('.fm-llm-consent input')).not.toBeNull(),
    );
    const consentSwitch = root.querySelector<HTMLInputElement>('.fm-llm-consent input');
    consentSwitch?.click();
    m.redraw.sync();

    expect(await save?.()).toBe(false);
    expect(createProfile).toHaveBeenCalledOnce();
    expect(grant).toHaveBeenCalledOnce();

    // The profile was recorded before the grant failed: a retry never creates a second one, and
    // it asks for consent again rather than saving silently.
    expect(await save?.()).toBe(false);
    expect(grant).toHaveBeenCalledOnce();
    m.redraw.sync();
    root.querySelector<HTMLInputElement>('.fm-llm-consent input')?.click();
    m.redraw.sync();
    expect(await save?.()).toBe(true);
    expect(createProfile).toHaveBeenCalledOnce();
    expect(updateProfile).not.toHaveBeenCalled();
    expect(await client.listLlmProfiles()).toHaveLength(1);
    expect(grant).toHaveBeenCalledTimes(2);
  });

  it('invalidates consent if a saved cloud endpoint path changes on the same host', async () => {
    const client = new MockFileManagerClient();
    const draft = {
      name: 'Cloud profile',
      preset: 'openAiCompatible' as const,
      baseUrl: 'https://llm.example.test:8443/first',
      deployment: null,
      apiVersion: null,
      model: 'model-a',
      credential: null,
      advanced: {
        contextWindow: 8192,
        maximumAnswerTokens: 1024,
        temperature: 0.2,
        timeoutSeconds: 30,
        tlsPolicy: 'requireHttps' as const,
        customHeaders: {},
      },
      capabilities: ['chatCompletions' as const],
      redactFilenames: true,
    };
    const profile = await client.createLlmProfile(draft);
    expect((await client.activateLlmProfile(profile.id, true)).consentedHost).toBe(
      'https://llm.example.test:8443/first',
    );
    const changed = await client.updateLlmProfile(profile.id, {
      ...draft,
      baseUrl: 'https://llm.example.test:8443/second',
    });
    expect(changed.consentedHost).toBeNull();
    await expect(client.activateLlmProfile(profile.id, false)).rejects.toMatchObject({
      code: 'invalidRequest',
    });
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
      expect(fieldValue('Model')).toBe('model-a');
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

    expect(fieldValue('Model')).toBe('model-b');
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
      expect(fieldValue('Model')).toBe('llama3.2:3b');
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true);
    });
    expect(root.textContent).not.toContain('Available provider models');
  });

  it('saves the visibly selected first discovered Ollama model without a change event', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'discoverLlmProfileDraftModels').mockResolvedValue([
      'llama3.2:3b',
      'qwen3:14b',
    ]);
    let save: (() => Promise<boolean>) | undefined;
    await mountLoaded(client, (handler) => {
      save = handler;
    });
    await vi.waitFor(() => {
      expect(fieldValue('Model')).toBe('llama3.2:3b');
      expect(fieldInput('Model')?.classList.contains('select-dropdown')).toBe(true);
    });
    if (!save) throw new Error('Settings did not register profile save');
    expect(await save()).toBe(true);
    expect(await client.listLlmProfiles()).toEqual([
      expect.objectContaining({ preset: 'ollama', model: 'llama3.2:3b' }),
    ]);
    expect((await client.getKnowledgeCapabilities()).answerGeneration).toBe(true);
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
