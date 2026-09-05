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

async function mountLoaded(client: MockFileManagerClient): Promise<void> {
  m.mount(root, { view: () => m(LlmProfileManagement, { client }) });
  await vi.waitFor(() => expect(root.textContent).not.toContain('Loading generation profiles'));
  m.redraw.sync();
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
});
