import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { EntrySummary } from '../../models';
import { DocumentSummaryDialog } from './document-summary-dialog';

let root: HTMLElement;

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

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('DocumentSummaryDialog', () => {
  it('labels representative evidence as key passages when no profile exists', async () => {
    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client: new MockFileManagerClient(),
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry,
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('A representative passage'));
    expect(root.textContent).toContain('No generation profile is configured');
    expect(root.textContent).toContain('Key passages');
  });

  it('discloses cloud transfer and generates a persisted summary', async () => {
    const client = new MockFileManagerClient();
    await client.createLlmProfile({
      name: 'Cloud profile',
      preset: 'openAiCompatible',
      baseUrl: 'https://llm.example.test',
      deployment: null,
      apiVersion: null,
      model: 'summary-model',
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
      redactFilenames: true,
    });
    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client,
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry,
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('cloud endpoint'));
    const generate = [...root.querySelectorAll<HTMLButtonElement>('button')].find((button) =>
      button.textContent?.includes('Generate summary'),
    );
    generate?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('A concise representative summary.'));
    expect(root.textContent).toContain('Full summary');
  });

  it('loads representative passages when no persisted summary exists', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getDocumentSummary').mockResolvedValue(null);

    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client,
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry,
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('A representative passage'));
    expect(root.textContent).not.toContain('The summary could not be loaded.');
  });

  it('surfaces generation endpoint errors instead of hiding them', async () => {
    const client = new MockFileManagerClient();
    await client.createLlmProfile({
      name: 'Local profile',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'summary-model',
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
    vi.spyOn(client, 'generateDocumentSummary').mockRejectedValue({
      code: 'providerUnavailable',
      message: 'Ollama is not reachable at http://127.0.0.1:11434',
    });
    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client,
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry,
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
          candidate.textContent?.includes('Generate summary'),
        ),
      ).toBeDefined(),
    );
    const generate = [...root.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      candidate.textContent?.includes('Generate summary'),
    );
    generate?.click();

    await vi.waitFor(() =>
      expect(root.textContent).toContain('Ollama is not reachable at http://127.0.0.1:11434'),
    );
  });
});
