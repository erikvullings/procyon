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
  vi.restoreAllMocks();
});

describe('RagAskDialog', () => {
  it('focuses the question when the dialog opens', async () => {
    const client = await configuredClient();
    let open = false;
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: ['source-1'],
          onClose: vi.fn(),
        }),
    });

    const previousFocus = document.createElement('button');
    root.appendChild(previousFocus);
    previousFocus.focus();
    open = true;
    m.redraw.sync();

    await vi.waitFor(() =>
      expect(document.activeElement).toBe(
        root.querySelector<HTMLTextAreaElement>('#fm-rag-question-input'),
      ),
    );
  });

  it('shows the profile, grounded default, and all five one-action scopes', async () => {
    const client = await configuredClient();
    const includeCurrentFolder = vi.fn();
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
          onIncludeCurrentFolder: includeCurrentFolder,
        }),
    });

    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));
    const dialog = root.querySelector('.fm-rag-ask-modal');
    expect(dialog?.classList.contains('fm-dense-modal')).toBe(false);
    expect(root.textContent).toContain('Ask your files');
    expect(root.querySelector<HTMLTextAreaElement>('.fm-rag-question textarea')?.rows).toBe(5);
    expect(root.querySelector<HTMLDetailsElement>('.fm-rag-options')?.open).toBe(false);
    expect(root.querySelector('.fm-rag-options')?.closest('.fm-rag-preferences')).not.toBeNull();
    expect(root.querySelector('.fm-rag-answer')?.textContent).toContain(
      'Your generated answer will appear here.',
    );
    expect(root.textContent).not.toContain('Enter to ask');
    expect(root.textContent).not.toContain('Generate answer');
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
    const preferences = root.querySelector('.fm-rag-preferences');
    const preferenceRow = preferences?.querySelector('.fm-rag-preference-row');
    expect(preferenceRow?.querySelector('.fm-rag-options')).not.toBeNull();
    const checkboxes = preferences?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(checkboxes).toHaveLength(2);
    const includeFolder = checkboxes?.item(0);
    expect(includeFolder?.checked).toBe(false);
    if (includeFolder === undefined) throw new Error('folder inclusion checkbox not rendered');
    includeFolder.checked = true;
    includeFolder.dispatchEvent(new Event('change', { bubbles: true }));
    expect(includeCurrentFolder).toHaveBeenCalledOnce();
    const modelKnowledge = checkboxes?.item(1);
    expect(modelKnowledge?.checked).toBe(false);
    expect(root.textContent).toContain('read-only');
    expect(root.textContent).toContain('Index current folder');
    expect(root.textContent).toContain('Allow model knowledge');
    expect(root.textContent).not.toContain('model-only claims');
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
    expect(root.querySelector('.fm-rag-evidence-score')?.textContent).toMatch(
      /^Similarity -?\d\.\d{3}$/,
    );
    expect(root.querySelector('[aria-labelledby="rag-evidence-heading"]')).not.toBeNull();
    expect(root.textContent).toContain('Coverage: 3/3 indexed');

    question.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));
    expect(root.querySelector('[aria-live="polite"]')).not.toBeNull();
    expect(root.textContent).toContain('C1');

    question.value = 'What evidence supports that?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    button('Preview evidence')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Mock indexed evidence'));
    question.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));

    button('Save conversation')?.click();
    await vi.waitFor(() => expect(root.textContent).toContain('Saved conversations'));
    expect(root.textContent).toContain('2 turn(s)');

    button('Delete')?.click();
    await vi.waitFor(() => expect(root.textContent).not.toContain('Saved conversations'));
  });

  it('submits with Enter and preserves Shift+Enter for a newline', async () => {
    const client = await configuredClient();
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open: true,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: [],
          onClose: vi.fn(),
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));

    const question = root.querySelector<HTMLTextAreaElement>('textarea');
    if (question === null) throw new Error('question input not rendered');
    question.value = 'What does the report conclude?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));

    const newline = new KeyboardEvent('keydown', {
      key: 'Enter',
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    question.dispatchEvent(newline);
    expect(newline.defaultPrevented).toBe(false);
    expect(root.textContent).not.toContain('Mock indexed evidence');

    const submit = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
    });
    question.dispatchEvent(submit);
    expect(submit.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));
  });

  it('renders sanitized Markdown and copies the raw question and answer', async () => {
    const client = await configuredClient();
    const originalPreview = client.previewRag.bind(client);
    vi.spyOn(client, 'previewRag').mockImplementation(async (request, signal) => {
      const response = await originalPreview(request, signal);
      return {
        ...response,
        evidence: response.evidence.map((evidence) => ({
          ...evidence,
          sourceId: 'mock-source-1',
          title: 'TRIZ%20Engineering%20of%20Creativity.pdf',
        })),
      };
    });
    const originalGenerate = client.generateRagAnswer.bind(client);
    vi.spyOn(client, 'generateRagAnswer').mockImplementation(async (request, signal) => {
      const response = await originalGenerate(request, signal);
      const text =
        '# SU-fields\n\nUse **substance-field analysis** [C1].\n\n<script>alert(1)</script>';
      return {
        ...response,
        events: response.events.map((event) => {
          if (event.type === 'token') return { ...event, text };
          if (event.type === 'done') {
            return {
              ...event,
              answer: {
                ...event.answer,
                text,
                citations: event.answer.citations.map((citation) => ({
                  ...citation,
                  provenance:
                    '{"kind":"exact","value":"{\\"block_index\\":0,\\"kind\\":\\"pdfBlock\\",\\"page_number\\":195}"}',
                })),
              },
            };
          }
          if (event.type === 'retrieval') {
            return {
              ...event,
              preview: {
                ...event.preview,
                evidence: event.preview.evidence.map((evidence) => ({
                  ...evidence,
                  title: 'TRIZ%20Engineering%20of%20Creativity.pdf',
                })),
              },
            };
          }
          return event;
        }),
      };
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open: true,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: [],
          onClose: vi.fn(),
          onOpenCitation: vi.fn(),
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));

    const question = root.querySelector<HTMLTextAreaElement>('textarea');
    if (question === null) throw new Error('question input not rendered');
    question.value = 'What explains SU-fields?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('[aria-label="Copy question"]')?.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('What explains SU-fields?'));

    question.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => expect(root.querySelector('.fm-rag-answer-markdown h1')).not.toBeNull());
    expect(root.textContent).toContain('TRIZ Engineering of Creativity.pdf');
    expect(root.querySelector('.fm-rag-answer-markdown strong')?.textContent).toBe(
      'substance-field analysis',
    );
    expect(root.querySelector('.fm-rag-answer-markdown script')).toBeNull();
    expect(root.querySelector<HTMLAnchorElement>('.fm-rag-answer-markdown a')?.textContent).toBe(
      'C1',
    );
    expect(root.querySelector('.fm-rag-citations')?.textContent).toContain('Page 195');
    expect(root.querySelector('.fm-rag-citations')?.textContent).not.toContain('pdfBlock');

    root.querySelector<HTMLButtonElement>('[aria-label="Copy answer"]')?.click();
    await vi.waitFor(() =>
      expect(writeText).toHaveBeenLastCalledWith(
        '# SU-fields\n\nUse **substance-field analysis** [C1].\n\n<script>alert(1)</script>',
      ),
    );
    root.querySelector<HTMLButtonElement>('[aria-label="Copy evidence C1"]')?.click();
    await vi.waitFor(() =>
      expect(writeText).toHaveBeenLastCalledWith(
        'Mock indexed evidence relevant to "What explains SU-fields?".',
      ),
    );
  });

  it('opens evidence and inline references and restores state until New question is chosen', async () => {
    const client = await configuredClient();
    const originalPreview = client.previewRag.bind(client);
    vi.spyOn(client, 'previewRag').mockImplementation(async (request, signal) => {
      const response = await originalPreview(request, signal);
      return {
        ...response,
        evidence: response.evidence.map((evidence) => ({
          ...evidence,
          sourceId: 'mock-source-1',
        })),
      };
    });
    const onOpenCitation = vi.fn().mockResolvedValue(undefined);
    let open = true;
    m.mount(root, {
      view: () =>
        m(RagAskDialog, {
          open,
          client,
          workspaceId,
          currentFolder: { providerId: 'local', uri: 'file:///documents' },
          selectedEntries: [entry],
          semanticSourceIds: [],
          onClose: () => {
            open = false;
          },
          onOpenCitation,
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Local profile'));

    const question = root.querySelector<HTMLTextAreaElement>('textarea');
    if (question === null) throw new Error('question input not rendered');
    question.value = 'What explains SU-fields?';
    question.dispatchEvent(new InputEvent('input', { bubbles: true }));
    question.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('grounded in the selected'));

    root.querySelector<HTMLButtonElement>('[aria-label="Open evidence C1"]')?.click();
    await vi.waitFor(() => expect(onOpenCitation).toHaveBeenCalledWith('mock-source-1'));
    root.querySelector<HTMLAnchorElement>('.fm-rag-answer-markdown a')?.click();
    await vi.waitFor(() => expect(onOpenCitation).toHaveBeenCalledTimes(2));

    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();
    await vi.waitFor(
      () =>
        expect(root.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe(
          'What explains SU-fields?',
        ),
      { timeout: 2_000 },
    );
    expect(root.textContent).toContain('grounded in the selected');

    root.querySelector<HTMLButtonElement>('[aria-label="New question"]')?.click();
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('textarea')?.value).toBe(''),
    );
    expect(root.textContent).not.toContain('grounded in the selected');
  });
});
