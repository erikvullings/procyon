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
    expect(root.textContent).toContain('Overview');
    expect(root.textContent).not.toContain('S1');
    expect(root.querySelector('.fm-document-summary-passages li p')?.textContent).toBe(
      'A representative passage from the selected document.',
    );
  });

  it('discloses cloud transfer and generates a persisted summary', async () => {
    const client = new MockFileManagerClient();
    const preview = vi.spyOn(client, 'previewDocumentSummary');
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
    const originalGenerate = client.generateDocumentSummary.bind(client);
    vi.spyOn(client, 'generateDocumentSummary').mockImplementation(async (request, signal) => ({
      ...(await originalGenerate(request, signal)),
      title: {
        value: 'Evidence review',
        source: 'aiExtracted',
        supportingChunkId: 'mock-chunk',
        provenance: '{"kind":"exact","value":{"kind":"pdfBlock","page_number":2,"block_index":1}}',
      },
      authors: [],
    }));
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

    await vi.waitFor(() => expect(root.textContent).toContain('whole extracted document'));
    expect(root.textContent).toContain('provider did not report a limit');
    expect(root.textContent).toContain('Cloud profile · summary-model · Cloud');
    expect(root.querySelector('.fm-document-summary-image-mode')).toBeNull();
    expect(preview).toHaveBeenLastCalledWith(expect.objectContaining({ inputTokenBudget: 6_144 }));
    const generate = [...root.querySelectorAll<HTMLButtonElement>('button')].find((button) =>
      button.textContent?.includes('Generate summary'),
    );
    generate?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('A concise document summary.'));
    expect(root.textContent).toContain('Evidence review · AI-extracted · page 2');
    expect(root.textContent).toContain('AuthorsUnavailable');
    expect(root.textContent).toContain('Full summary');
  });

  it('shows an accessible skeleton only during summary generation', async () => {
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
    const original = client.generateDocumentSummary.bind(client);
    let finish: (() => void) | undefined;
    vi.spyOn(client, 'generateDocumentSummary').mockImplementation(async (request, signal) => {
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
      return original(request, signal);
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
    await vi.waitFor(() => expect(root.textContent).toContain('whole extracted document'));
    expect(root.querySelector('.fm-document-summary-progress')).toBeNull();
    const generate = [...root.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      candidate.textContent?.includes('Generate summary'),
    );
    generate?.click();
    m.redraw.sync();
    const progress = root.querySelector('.fm-document-summary-progress');
    expect(progress?.getAttribute('role')).toBe('status');
    expect(progress?.getAttribute('aria-busy')).toBe('true');
    expect(progress?.textContent).toContain('Generating summary');
    expect(progress?.querySelectorAll('.fm-document-summary-skeleton .mm-skeleton')).toHaveLength(
      4,
    );
    expect(progress?.querySelector('.mm-skeleton-group')?.getAttribute('aria-hidden')).toBe('true');
    finish?.();
    await vi.waitFor(() => expect(root.querySelector('.fm-document-summary-progress')).toBeNull());
    expect(root.textContent).toContain('A concise document summary.');
  });

  it('uses the whole source with a visible multi-pass warning instead of a representative-only switch', async () => {
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
    const preview = vi.spyOn(client, 'previewDocumentSummary');
    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client,
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry: {
            ...entry,
            location: { providerId: 'local', uri: 'file:///documents/large-report.txt' },
          },
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('2 bounded source-reading passes'));
    expect(root.querySelector('.fm-document-summary-input-mode')).toBeNull();
    expect(preview).toHaveBeenLastCalledWith(expect.objectContaining({ inputTokenBudget: 6_144 }));
  });

  it('offers images only for a vision-capable document and keeps them off by default', async () => {
    const client = new MockFileManagerClient();
    await client.createLlmProfile({
      name: 'Vision profile',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      deployment: null,
      apiVersion: null,
      model: 'vision-model',
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
    const preview = vi.spyOn(client, 'previewDocumentSummary');
    m.mount(root, {
      view: () =>
        m(DocumentSummaryDialog, {
          open: true,
          client,
          workspaceId: '22222222-2222-4222-8222-222222222222',
          entry: {
            ...entry,
            name: 'report.docx',
            extension: 'docx',
            location: { providerId: 'local', uri: 'file:///documents/report.docx' },
          },
          onClose: vi.fn(),
        }),
    });

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-document-summary-image-mode')).not.toBeNull(),
    );
    const includeImages = root.querySelector<HTMLInputElement>(
      '.fm-document-summary-image-mode input',
    );
    expect(includeImages?.checked).toBe(false);
    includeImages?.click();

    await vi.waitFor(() => expect(root.textContent).toContain('1 embedded image'));
    expect(preview).toHaveBeenLastCalledWith(expect.objectContaining({ includeImages: true }));
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
    expect(root.querySelector('.fm-document-summary-image-mode')).toBeNull();
    const generate = [...root.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      candidate.textContent?.includes('Generate summary'),
    );
    generate?.click();

    await vi.waitFor(() =>
      expect(root.textContent).toContain('Ollama is not reachable at http://127.0.0.1:11434'),
    );
  });
});
