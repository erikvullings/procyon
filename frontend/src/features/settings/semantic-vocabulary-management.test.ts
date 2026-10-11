import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { SemanticVocabulary } from '../../models';
import { SemanticVocabularyManagement } from './semantic-vocabulary-management';

let root: HTMLElement;
let client: MockFileManagerClient;

const vocabulary: SemanticVocabulary = {
  id: 'research',
  name: 'Research',
  concepts: [
    {
      uri: 'https://example.test/ml',
      prefLabels: { en: 'Machine learning' },
      altLabels: {},
      definitions: {},
      scopeNotes: {},
      broader: [],
      narrower: [],
      related: [],
      extensions: {},
    },
  ],
  workspaceIds: [],
  rootIds: [],
  reviewQueue: [
    {
      id: 'candidate-rag',
      label: 'Retrieval augmented generation',
      synonyms: ['RAG'],
      supportingChunkIds: ['chunk-1'],
      confidence: 0.81,
      corpusFrequency: 5,
      status: 'pending',
    },
  ],
  revision: 1,
};

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
  client = new MockFileManagerClient();
  vi.spyOn(client, 'listSemanticVocabularies').mockResolvedValue([vocabulary]);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('SemanticVocabularyManagement', () => {
  const mount = () => {
    m.mount(root, {
      view: () =>
        m(SemanticVocabularyManagement, {
          client,
          workspaceId: 'workspace-a',
        }),
    });
  };

  it('shows imported concepts and review evidence without offering exact folders', async () => {
    const attached = { ...vocabulary, workspaceIds: ['workspace-a'] };
    vi.mocked(client.listSemanticVocabularies).mockResolvedValue([attached]);
    m.mount(root, {
      view: () =>
        m(SemanticVocabularyManagement, {
          client,
          workspaceId: 'workspace-a',
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Machine learning'));
    expect(root.textContent).toContain('81% confidence across 5 occurrences');

    expect(root.textContent).not.toContain('Create concept folder');
    expect(root.querySelector('#semantic-vocabulary-starter')).toBeNull();
  });

  it('imports a user vocabulary without attaching it', async () => {
    const importSpy = vi.spyOn(client, 'importSemanticVocabulary');
    const attachSpy = vi.spyOn(client, 'attachSemanticVocabulary');
    mount();
    await vi.waitFor(() => expect(root.querySelector('textarea')?.disabled).toBeFalsy());
    expect(importSpy).not.toHaveBeenCalled();
    const input = root.querySelector<HTMLTextAreaElement>('textarea');
    if (!input) throw new Error('Missing import input');
    input.value = JSON.stringify({
      format: 'procyon-skos-1',
      id: 'custom',
      name: 'Custom',
      concepts: [{ uri: 'urn:test:custom', prefLabels: { en: 'Custom' } }],
    });
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find(({ textContent }) => textContent === 'Import vocabulary')
      ?.click();
    await vi.waitFor(() => expect(importSpy).toHaveBeenCalledOnce());
    const imported = JSON.parse(importSpy.mock.calls[0]?.[0] ?? '{}');
    expect(imported.id).toBe('custom');
    await vi.waitFor(() => expect(root.textContent).toContain('Custom'));
    expect(root.textContent).toContain('Attach to workspace');
    expect(attachSpy).not.toHaveBeenCalled();
    expect(client.listSemanticVocabularies).toHaveBeenCalledOnce();
  });

  it('surfaces import failures', async () => {
    const importSpy = vi.spyOn(client, 'importSemanticVocabulary');
    mount();
    await vi.waitFor(() => expect(root.textContent).toContain('Machine learning'));
    const input = root.querySelector<HTMLTextAreaElement>('textarea');
    if (!input) throw new Error('Missing import input');
    input.value = '{}';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    importSpy.mockRejectedValueOnce(new Error('Import denied'));
    m.redraw.sync();
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find(({ textContent }) => textContent === 'Import vocabulary')
      ?.click();
    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toBe('Import denied'),
    );
    expect(input.value).toBe('{}');
  });
});
