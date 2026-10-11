import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { SemanticVocabulary } from '../../models';
import { SemanticVocabularyManagement } from './semantic-vocabulary-management';
import { skosStarters } from './skos-starters';

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
          onCreateConceptFolder: vi.fn(),
        }),
    });
  };

  const chooseStarter = async (id: string) => {
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLSelectElement>('select')?.disabled).toBe(false),
    );
    const select = root.querySelector<HTMLSelectElement>('select');
    if (!select) throw new Error('Missing starter chooser');
    select.value = id;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();
  };

  const importStarter = () => {
    const button = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      ({ textContent }) => textContent === 'Import selected starter',
    );
    if (!button) throw new Error('Missing starter import button');
    button.click();
  };

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

  it('imports an attributed starter only on request, without attaching it', async () => {
    const importSpy = vi.spyOn(client, 'importSemanticVocabulary');
    const attachSpy = vi.spyOn(client, 'attachSemanticVocabulary');
    mount();
    await chooseStarter('unesco-information-technology');
    expect(importSpy).not.toHaveBeenCalled();
    expect(root.textContent).toContain('UNESCO');
    expect(root.textContent).toContain('CC BY-SA 3.0 IGO');
    expect(root.querySelector<HTMLAnchorElement>('a[href*="unesco-thesaurus.rdf"]')?.href).toBe(
      skosStarters[2].sourceUrl,
    );
    expect(root.querySelector<HTMLAnchorElement>('a[href*="by-sa/3.0/igo"]')?.href).toBe(
      skosStarters[2].licenseUrl,
    );

    importStarter();
    await vi.waitFor(() => expect(importSpy).toHaveBeenCalledOnce());
    const imported = JSON.parse(importSpy.mock.calls[0]?.[0] ?? '{}');
    expect(imported.id).toBe('unesco-information-technology');
    expect(imported.concepts).toHaveLength(98);
    expect(imported.extensions['https://procyon.app/ns/skos-starter/source']).toBe(
      skosStarters[2].sourceUrl,
    );
    await vi.waitFor(() => expect(root.textContent).toContain(imported.name));
    expect(root.textContent).toContain('Attach to workspace');
    expect(attachSpy).not.toHaveBeenCalled();
    expect(client.listSemanticVocabularies).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(root.querySelector<HTMLSelectElement>('select')?.value).toBe(''));
  });

  it('does not reimport an existing starter and surfaces import failures', async () => {
    const importSpy = vi.spyOn(client, 'importSemanticVocabulary');
    mount();
    await chooseStarter('unesco-psychology');
    importSpy.mockRejectedValueOnce(new Error('Import denied'));
    importStarter();
    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toBe('Import denied'),
    );
    expect(root.querySelector<HTMLSelectElement>('select')?.value).toBe('unesco-psychology');

    importStarter();
    await vi.waitFor(() =>
      expect(root.textContent).toContain(
        'Psychology (UNESCO Thesaurus 2026-08-31 snapshot excerpt)',
      ),
    );
    await chooseStarter('unesco-psychology');
    expect(root.textContent).toContain('Already imported');
    expect(
      [...root.querySelectorAll<HTMLButtonElement>('button')].find(({ textContent }) =>
        textContent?.includes('Already imported'),
      )?.disabled,
    ).toBe(true);
    expect(importSpy).toHaveBeenCalledTimes(2);
  });
});
