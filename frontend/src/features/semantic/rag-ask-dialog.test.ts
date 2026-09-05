import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { EntrySummary } from '../../models';
import { RagAskDialog } from './rag-ask-dialog';

let root: HTMLElement;

const workspaceId = '22222222-2222-4222-8222-222222222222';
const entry: EntrySummary = {
  id: '11111111-1111-4111-8111-111111111111',
  location: { providerId: 'local', uri: 'file:///documents/report.txt' },
  name: 'report.txt',
  kind: 'file',
  size: 128,
  hidden: false,
  readOnly: false,
  extension: 'txt',
  mimeType: 'text/plain',
  metadataRevision: 1,
};

async function configuredClient(): Promise<MockFileManagerClient> {
  const client = new MockFileManagerClient();
  await client.createLlmProfile({
    name: 'Local profile',
    preset: 'ollama',
    baseUrl: 'http://127.0.0.1:11434',
    deployment: null,
    apiVersion: null,
    model: 'ask-model',
    credential: null,
    advanced: {
      contextWindow: 8_192,
      maximumAnswerTokens: 1_024,
      temperature: 0.2,
      timeoutSeconds: 30,
      tlsPolicy: 'requireValidCertificate',
      customHeaders: {},
    },
    capabilities: ['chatCompletions'],
    redactFilenames: false,
  });
  return client;
}

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('RagAskDialog', () => {
  it('shows the profile, grounded default, and all five one-action scopes', async () => {
    const client = await configuredClient();
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open: true,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: ['source-1'],
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));
    const selects = root.querySelectorAll<HTMLSelectElement>('select');
    expect(selects).toHaveLength(2);
    const scopeSelect = selects.item(1);
    expect([...scopeSelect.options].map((option) => option.text)).toEqual([
      'Entire indexed library',
      'Selected files',
      'Current folder recursively',
      'Current semantic result set',
      'Named enrolled roots',
    ]);
    expect(scopeSelect.value).toBe('entireLibrary');
    const modelKnowledge = root.querySelector<HTMLInputElement>('input[type="checkbox"]');
    expect(modelKnowledge?.checked).toBe(false);
    expect(root.textContent).toContain('read-only');
  });

  it('previews evidence before generation and explicitly saves and deletes a conversation', async () => {
    const client = await configuredClient();
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open: true,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: ['source-1'],
          onClose: vi.fn(),
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));

    const question = root.querySelector<HTMLTextAreaElement>('textarea');
    if (question === null) throw new Error('question input not rendered');
    question.value = 'What does the report conclude?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    const button = (label: string) =>
      [...root.querySelectorAll<HTMLButtonElement>('button')].find((item) =>
        item.textContent?.includes(label),
      );

    button('Preview evidence')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Mock indexed evidence'));
    expect(root.querySelector('[aria-labelledby="rag-evidence-heading"]')).not.toBeNull();
    expect(root.textContent).toContain('Coverage: 3/3 indexed');

    button('Generate answer')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));
    expect(root.querySelector('[aria-live="polite"]')).not.toBeNull();
    expect(root.textContent).toContain('C1');

    question.value = 'What evidence supports that?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    button('Preview evidence')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Mock indexed evidence'));
    button('Generate answer')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));

    button('Save conversation')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Saved conversations'));
    expect(root.textContent).toContain('2 turn(s)');

    button('Delete')?.click();
    await vi.waitFor(() => expect(root.textContent).not.toContain('Saved conversations'));
  });
});
