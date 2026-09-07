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
  it('shows curated concepts, review evidence, and stable-folder action', async () => {
    const onCreateConceptFolder = vi.fn();
    m.mount(root, {
      view: () =>
        m(SemanticVocabularyManagement, {
          client,
          workspaceId: 'workspace-a',
          onCreateConceptFolder,
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Machine learning'));
    expect(root.textContent).toContain('81% confidence across 5 occurrences');

    const create = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      ({ textContent }) => textContent === 'Create concept folder',
    );
    create?.click();
    expect(onCreateConceptFolder).toHaveBeenCalledWith(vocabulary, 'https://example.test/ml');
  });
});
