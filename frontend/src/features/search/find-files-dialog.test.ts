import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { FindFilesSearchParams } from './find-files-dialog';
import { FindFilesDialog } from './find-files-dialog';

let root: HTMLElement;

beforeEach(() => {
  localStorage.clear();
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('FindFilesDialog', () => {
  async function indexedClient(options?: {
    indexedSources?: number;
    workspaceId?: string;
  }): Promise<MockFileManagerClient> {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const status = await client.getSemanticLibraryStatus();
    const [indexed] = await client.listKnowledgeRoots({ workspaceId: 'workspace-a' });
    if (!indexed) throw new Error('Missing indexed mock root');
    vi.spyOn(client, 'getSemanticLibraryStatus').mockResolvedValue({
      ...status,
      roots: [
        {
          id: indexed.rootId,
          location: indexed.location,
          recursive: true,
          stableIdentityVerified: true,
          workspaceReferences: [options?.workspaceId ?? 'workspace-a'],
          eligibilityOverrides: [],
          attachedVocabularyIds: [],
          eligibilityReasonCounts: [],
          ocrRequiredFiles: [],
          availability: { state: 'available' },
          reconciliationGeneration: 4,
          indexedGeneration: 4,
          exclusions: [],
        },
      ],
    });
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([
      { ...indexed, indexedSources: options?.indexedSources ?? 3 },
    ]);
    return client;
  }

  it.each([
    ['disabled components', async () => new MockFileManagerClient()],
    [
      'no indexed roots',
      async () => new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' }),
    ],
    ['no indexed documents', async () => indexedClient({ indexedSources: 0 })],
    ['another workspace root', async () => indexedClient({ workspaceId: 'workspace-b' })],
  ])('hides the concept tab with %s', async (_reason, createClient) => {
    const client = await createClient();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(client.getSemanticComponentStatus).toBeDefined());
    expect(document.querySelector('.fm-find-files-modes')).toBeNull();
    expect(document.querySelector('#find-files-query')).not.toBeNull();
  });

  it('surfaces availability errors while leaving file search usable', async () => {
    const client = await indexedClient();
    vi.spyOn(client, 'getSemanticComponentStatus').mockRejectedValue(
      new Error('Status unavailable'),
    );
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(root.textContent).toContain('Status unavailable'));
    expect(document.querySelector('.fm-find-files-modes')).toBeNull();
    expect(document.querySelector('#find-files-query')).not.toBeNull();
  });

  it('searches unattached imported concepts and suggests recent selections', async () => {
    const client = await indexedClient();
    await client.importSemanticVocabulary(
      JSON.stringify({
        format: 'procyon-skos-1',
        id: 'topics',
        name: 'Topics',
        concepts: [
          {
            uri: 'https://example.test/ml',
            prefLabels: { en: 'Machine learning' },
            definitions: { en: 'Learning from examples' },
          },
        ],
      }),
    );
    const listVocabularies = vi.spyOn(client, 'listSemanticVocabularies');
    const getLibrary = vi.spyOn(client, 'getSemanticLibraryStatus');
    const onSearch = vi.fn();
    const onSave = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          savedSearches: [],
          onSearch,
          onSave,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    expect(listVocabularies).not.toHaveBeenCalled();
    expect(getLibrary).toHaveBeenCalledOnce();
    await vi.waitFor(() =>
      expect(
        document.querySelector('.fm-find-files-modes button[aria-pressed="false"]')?.textContent,
      ).toBe('Find by concept'),
    );
    [...document.querySelectorAll<HTMLButtonElement>('.fm-find-files-modes button')]
      .find((button) => button.textContent === 'Find by concept')
      ?.click();
    m.redraw.sync();
    expect(document.querySelector('.fm-find-concept-list button')).toBeNull();
    const filter = document.querySelector<HTMLInputElement>('#find-files-concept');
    if (!filter) throw new Error('concept filter missing');
    filter.value = 'machine';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(
        [...document.querySelectorAll<HTMLButtonElement>('.fm-find-concept-list button')].some(
          (button) => button.textContent?.includes('Machine learning · Topics'),
        ),
      ).toBe(true),
    );
    const choice = [
      ...document.querySelectorAll<HTMLButtonElement>('.fm-find-concept-list button'),
    ].find((button) => button.textContent?.includes('Machine learning · Topics'));
    expect(choice?.textContent).toContain('Machine learning');
    expect(choice?.textContent).toContain('Learning from examples');
    choice?.click();
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-find-concept-scope input')?.checked).toBe(
      false,
    );
    [...document.querySelectorAll<HTMLButtonElement>('.modal-footer button')]
      .find((button) => button.textContent?.trim() === 'Search')
      ?.click();
    await vi.waitFor(() => expect(onSearch).toHaveBeenCalledOnce());
    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        mode: 'semantic',
        semanticQuery: 'Machine learning. Learning from examples',
        semanticLibraryId: (await client.getSemanticLibraryStatus()).library?.libraryId,
        semanticScope: 'entireLibrary',
      }),
    );
    expect(localStorage.getItem('procyon.recentConceptSearches.v1')).toContain(
      'https://example.test/ml',
    );

    m.mount(root, null);
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          savedSearches: [],
          onSearch,
          onSave,
          onCancel: vi.fn(),
        }),
    });

    m.redraw.sync();
    await vi.waitFor(() => expect(document.querySelector('.fm-find-files-modes')).not.toBeNull());
    [...document.querySelectorAll<HTMLButtonElement>('.fm-find-files-modes button')]
      .find((button) => button.textContent === 'Find by concept')
      ?.click();
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(
        [...document.querySelectorAll<HTMLButtonElement>('.fm-find-concept-list button')].some(
          (button) => button.textContent?.includes('Machine learning · Topics'),
        ),
      ).toBe(true),
    );
    [...document.querySelectorAll<HTMLButtonElement>('.fm-find-concept-list button')]
      .find((button) => button.textContent?.includes('Machine learning · Topics'))
      ?.click();
    document.querySelector<HTMLInputElement>('.fm-find-concept-scope input')?.click();
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-find-concept-scope input')?.checked).toBe(
      true,
    );
    expect(document.querySelector('.fm-save-search-button')?.textContent).toBe(
      'Save as smart folder',
    );
    document.querySelector<HTMLButtonElement>('.fm-save-search-button')?.click();
    expect(onSave).toHaveBeenCalledWith(
      'Machine learning (Topics)',
      expect.objectContaining({ semanticScope: 'currentFolder' }),
      undefined,
    );
  });

  it('offers bundled concepts with source and license without importing them', async () => {
    const client = await indexedClient();
    const importSpy = vi.spyOn(client, 'importSemanticVocabulary');
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(root.querySelector('.fm-find-files-modes')).not.toBeNull());
    [...root.querySelectorAll<HTMLButtonElement>('.fm-find-files-modes button')]
      .find((button) => button.textContent === 'Find by concept')
      ?.click();
    m.redraw.sync();
    const filter = root.querySelector<HTMLInputElement>('#find-files-concept');
    if (!filter) throw new Error('Concept filter missing');
    filter.value = 'renewable energy';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-find-concept-choice')?.textContent).toContain(
        'renewable energy',
      ),
    );
    expect(root.querySelector('.fm-find-concept-source')?.textContent).toContain('CC BY 4.0');
    expect(root.querySelector('.fm-find-concept-source a')).not.toBeNull();
    root.querySelector<HTMLButtonElement>('.fm-find-concept-choice')?.click();
    m.redraw.sync();
    [...root.querySelectorAll<HTMLButtonElement>('.modal-footer button')]
      .find((button) => button.textContent?.trim() === 'Search')
      ?.click();
    expect(onSearch).toHaveBeenCalledOnce();
    expect(importSpy).not.toHaveBeenCalled();
  });

  it('names the selected UNESCO concept concisely without losing its identity', async () => {
    const client = await indexedClient();
    const onSearch = vi.fn();
    const onSave = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          savedSearches: [],
          onSearch,
          onSave,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(root.querySelector('.fm-find-files-modes')).not.toBeNull());
    [...root.querySelectorAll<HTMLButtonElement>('.fm-find-files-modes button')]
      .find((button) => button.textContent === 'Find by concept')
      ?.click();
    m.redraw.sync();
    const filter = root.querySelector<HTMLInputElement>('#find-files-concept');
    if (!filter) throw new Error('Concept filter missing');
    filter.value = 'Mental stress';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-find-concept-choice')?.textContent).toContain(
        'Mental stress · Psychology (UNESCO',
      ),
    );
    root.querySelector<HTMLButtonElement>('.fm-find-concept-choice')?.click();
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-saved-search-save input')?.value).toBe(
      'Mental stress (UNESCO)',
    );
    root.querySelector<HTMLButtonElement>('.fm-save-search-button')?.click();
    expect(onSave).toHaveBeenCalledWith(
      'Mental stress (UNESCO)',
      expect.objectContaining({
        conceptLabel: 'Mental stress (UNESCO)',
        selectedConcept: {
          vocabularyId: 'unesco-psychology',
          conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
        },
      }),
      undefined,
    );
    [...root.querySelectorAll<HTMLButtonElement>('.modal-footer button')]
      .find((button) => button.textContent?.trim() === 'Search')
      ?.click();
    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        conceptLabel: 'Mental stress (UNESCO)',
        selectedConcept: {
          vocabularyId: 'unesco-psychology',
          conceptUri: 'http://vocabularies.unesco.org/thesaurus/concept13013',
        },
      }),
    );
  });

  it('filters concept choices by vocabulary and submits only a choice from that vocabulary', async () => {
    const client = await indexedClient();
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          client,
          workspaceId: 'workspace-a',
          scopeLabel: '/Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(root.querySelector('.fm-find-files-modes')).not.toBeNull());
    [...root.querySelectorAll<HTMLButtonElement>('.fm-find-files-modes button')]
      .find((button) => button.textContent === 'Find by concept')
      ?.click();
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(
        root.querySelectorAll('select[name="concept-vocabulary"] option').length,
      ).toBeGreaterThan(2),
    );
    const vocabulary = root.querySelector<HTMLSelectElement>('select[name="concept-vocabulary"]');
    const filter = root.querySelector<HTMLInputElement>('#find-files-concept');
    if (!vocabulary || !filter) throw new Error('concept controls missing');
    expect(vocabulary.options[0]?.textContent).toBe('All vocabularies');
    expect(
      vocabulary.compareDocumentPosition(filter) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    filter.value = 'energy';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    const all = [...root.querySelectorAll<HTMLButtonElement>('.fm-find-concept-choice')];
    expect(all.length).toBeGreaterThan(1);
    const chosen = all.find((button) => button.textContent?.includes('renewable energy'));
    const sourceName = chosen?.querySelector('span')?.textContent?.replace(/^ · /u, '');
    const source = [...vocabulary.options].find((option) => option.textContent === sourceName);
    if (!chosen || !source) throw new Error('bundled energy concept missing');
    chosen.click();
    m.redraw.sync();
    vocabulary.value = source.value;
    vocabulary.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-find-concept-choice[aria-pressed="true"]')).toBeNull();
    expect(
      [...root.querySelectorAll('.fm-find-concept-choice')].every((choice) =>
        choice.textContent?.includes(` · ${sourceName}`),
      ),
    ).toBe(true);
    vocabulary.value = '';
    vocabulary.dispatchEvent(new Event('change', { bubbles: true }));
    filter.value = 'energy';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelectorAll('.fm-find-concept-choice').length).toBe(all.length);
    vocabulary.value = source.value;
    vocabulary.dispatchEvent(new Event('change', { bubbles: true }));
    filter.value = 'no matching concept exists';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-find-concept-list .fm-find-concept-empty')?.textContent).toBe(
      'No matching concept in the available vocabularies.',
    );
    filter.value = 'energy';
    filter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('.fm-find-concept-choice')?.click();
    m.redraw.sync();
    [...root.querySelectorAll<HTMLButtonElement>('.modal-footer button')]
      .find((button) => button.textContent?.trim() === 'Search')
      ?.click();
    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        mode: 'semantic',
        conceptLabel: expect.stringMatching(/\(GEMET\)$/u),
        semanticScope: 'entireLibrary',
      }),
    );
  });

  it('is focused on open and submits the trimmed query with Enter', async () => {
    const onSearch = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel,
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    expect(document.activeElement).toBe(input);
    if (!input) throw new Error('input missing');
    expect(input).toBeInstanceOf(HTMLInputElement);
    expect(input.type).toBe('text');
    expect(input.classList).toContain('browser-default');

    input.value = '  report  ';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).toHaveBeenCalledWith({
      mode: 'name',
      filenameQuery: 'report',
      contentRegex: false,
      contentCaseSensitive: false,
      contentWholeWord: false,
      recurse: true,
    });

    expect(document.activeElement).not.toBe(input);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('shows filename and content search together without mode tabs', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('#find-files-query')).not.toBeNull();
    expect(document.querySelector('#find-files-content-query')).not.toBeNull();
    expect(document.querySelector('.fm-find-files-modes')).toBeNull();
    expect(root.textContent).not.toContain('Semantic');
  });

  it('does not search on an empty/whitespace-only query', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });

    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');

    input.value = '   ';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).not.toHaveBeenCalled();
  });

  it('uses the filename query as the default saved-search name', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const query = document.querySelector<HTMLInputElement>('#find-files-query');
    const savedName = document.querySelector<HTMLInputElement>(
      'input[placeholder="Saved search name"]',
    );
    if (!query || !savedName) throw new Error('search inputs missing');

    query.value = '*.md, *.pdf';
    query.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(savedName.value).toBe('*.md, *.pdf');
  });

  it('leaves native text-editing shortcuts to the filename input', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');
    const documentKeydown = vi.fn();
    document.addEventListener('keydown', documentKeydown);

    for (const event of [
      new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'a', metaKey: true, bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'c', metaKey: true, bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'x', metaKey: true, bubbles: true, cancelable: true }),
    ]) {
      input.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }

    expect(documentKeydown).not.toHaveBeenCalled();
    document.removeEventListener('keydown', documentKeydown);
  });

  it('searches when only an advanced predicate is configured', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const minimum = document.querySelector<HTMLInputElement>('input[type="number"]');
    if (!minimum) throw new Error('minimum size input missing');
    minimum.value = '1024';
    minimum.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    const searchButton = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'Search',
    );
    expect(searchButton?.disabled).toBe(false);
    searchButton?.click();

    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({ filenameQuery: '', minSizeBytes: 1024 }),
    );
  });

  it('converts the selected size units to bytes', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const minimum = document.querySelector<HTMLInputElement>('#find-files-min-size');
    const unit = minimum
      ?.closest('.fm-find-files-size-control')
      ?.querySelector<HTMLSelectElement>('select');
    if (!minimum || !unit) throw new Error('minimum size controls missing');
    minimum.value = '1.5';
    minimum.dispatchEvent(new InputEvent('input', { bubbles: true }));
    unit.value = 'MB';
    unit.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();

    const searchButton = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'Search',
    );
    searchButton?.click();

    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({ filenameQuery: '', minSizeBytes: 1_500_000 }),
    );
  });

  it('uses compact native date inputs with explicit clear buttons', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const modifiedAfter = document.querySelector<HTMLInputElement>('#find-files-modified-after');
    expect(modifiedAfter?.type).toBe('date');
    expect(modifiedAfter?.classList).toContain('browser-default');
    if (!modifiedAfter) throw new Error('modified-after input missing');

    modifiedAfter.value = '2026-09-10';
    modifiedAfter.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    const clearButton = document.querySelector<HTMLButtonElement>(
      'button[aria-label="Clear modified-after date"]',
    );
    expect(clearButton).not.toBeNull();
    expect(clearButton?.querySelector('svg')).not.toBeNull();
    expect(clearButton?.textContent).toBe('');
    clearButton?.click();
    m.redraw.sync();

    expect(document.querySelector<HTMLInputElement>('#find-files-modified-after')?.value).toBe('');
  });

  it('keeps results out of the modal because they render in the active pane', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('.fm-find-files-results')).toBeNull();
    expect(document.querySelector('.fm-find-files-modal')?.id).toBe('find-files-dialog');
    expect(
      [...document.querySelectorAll('.fm-find-files-modal .modal-footer button')].every((button) =>
        button.classList.contains('btn-flat'),
      ),
    ).toBe(true);
  });

  it('blurs before cancel when Cancel is clicked', () => {
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel,
        }),
    });
    m.redraw.sync();
    const buttons = [...document.querySelectorAll('button')];
    const cancelButton = buttons.find((button) => button.textContent?.trim() === 'Cancel');
    if (!cancelButton) throw new Error('cancel button missing');

    cancelButton.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('keeps the previous query on reopen, fully selected, so typing replaces it and Enter re-searches', () => {
    let open = true;
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');

    input.value = '*.svg';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const expected: FindFilesSearchParams = {
      mode: 'name',
      filenameQuery: '*.svg',
      contentRegex: false,
      contentCaseSensitive: false,
      contentWholeWord: false,
      recurse: true,
    };
    expect(onSearch).toHaveBeenCalledWith(expected);

    // Simulate the parent closing the dialog after a successful search, then reopening it
    // for a second search (e.g. via Alt+F7 again).
    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();

    expect(input.value).toBe('*.svg');
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);

    // Pressing Enter immediately re-runs the same search.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSearch).toHaveBeenCalledTimes(2);
    expect(onSearch).toHaveBeenLastCalledWith(expected);
  });

  it('passes content query and options when content search is used', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const contentInput = document.querySelector<HTMLInputElement>('#find-files-content-query');
    if (!contentInput) throw new Error('content input missing');
    contentInput.value = 'TODO';
    contentInput.dispatchEvent(new InputEvent('input', { bubbles: true }));

    const files = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!files) throw new Error('filename input missing');
    files.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        filenameQuery: '',
        contentQuery: 'TODO',
        mode: 'content',
        contentRegex: false,
        recurse: true,
      }),
    );
  });

  it('shows VS Code-style content options and toggles their pressed states', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const matchCase = document.querySelector<HTMLButtonElement>('button[title="Match case"]');
    const matchWholeWord = document.querySelector<HTMLButtonElement>(
      'button[title="Match whole word"]',
    );
    const useRegex = document.querySelector<HTMLButtonElement>(
      'button[title="Use regular expression"]',
    );
    expect(matchCase?.textContent).toBe('Aa');
    expect(matchWholeWord?.textContent).toBe('Ab');
    expect(useRegex?.textContent).toBe('.*');
    expect(matchCase?.getAttribute('aria-pressed')).toBe('false');
    expect(matchWholeWord?.getAttribute('aria-pressed')).toBe('false');
    expect(useRegex?.getAttribute('aria-pressed')).toBe('false');

    matchCase?.click();
    matchWholeWord?.click();
    useRegex?.click();
    m.redraw.sync();

    expect(matchCase?.getAttribute('aria-pressed')).toBe('true');
    expect(matchWholeWord?.getAttribute('aria-pressed')).toBe('true');
    expect(useRegex?.getAttribute('aria-pressed')).toBe('true');

    const contentInput = document.querySelector<HTMLInputElement>('#find-files-content-query');
    if (!contentInput) throw new Error('content input missing');
    contentInput.value = 'TODO';
    contentInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    contentInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        contentCaseSensitive: true,
        contentWholeWord: true,
        contentRegex: true,
      }),
    );
  });

  it('places an accessible subdirectory toggle beside advanced filters', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const advancedSection = document.querySelector('.fm-find-files-advanced-section');
    const toggle = advancedSection?.querySelector<HTMLButtonElement>(
      '.fm-find-files-recurse-toggle',
    );
    expect(toggle?.title).toBe('Recurse subdirectories');
    expect(toggle?.getAttribute('aria-label')).toBe('Include subdirectories: On');
    expect(toggle?.getAttribute('aria-pressed')).toBe('true');
    expect(toggle?.querySelector('.fm-icon-directory-tree')).not.toBeNull();

    const details = document.querySelector<HTMLDetailsElement>('.fm-find-files-advanced');
    expect(details?.querySelector('.fm-find-files-advanced-caret')).not.toBeNull();
    expect(details?.open).toBe(false);
    details?.querySelector<HTMLElement>('summary')?.click();
    expect(details?.open).toBe(true);

    toggle?.click();
    m.redraw.sync();

    const updatedToggle = document.querySelector<HTMLButtonElement>(
      '.fm-find-files-recurse-toggle',
    );
    expect(updatedToggle?.getAttribute('aria-label')).toBe('Include subdirectories: Off');
    expect(updatedToggle?.getAttribute('aria-pressed')).toBe('false');
  });

  it('opens a saved search in either pane or a new tab and exposes pin/delete controls', () => {
    const onOpenSaved = vi.fn();
    const onToggleSavedPin = vi.fn();
    const onDeleteSaved = vi.fn();
    const saved = {
      id: '11111111-1111-4111-8111-111111111111',
      name: 'Large videos',
      pinned: true,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Videos' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: ['video/*'],
        minSizeBytes: 1_000_000,
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    };
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [saved],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
          onOpenSaved,
          onToggleSavedPin,
          onDeleteSaved,
        }),
    });
    m.redraw.sync();

    const action = (label: string) =>
      document.querySelector<HTMLButtonElement>(`.fm-saved-search [aria-label="${label}"]`);
    action('Open in current pane')?.click();
    action('Open in other pane')?.click();
    action('Open in new tab')?.click();
    action('Remove from favourites')?.click();
    action('Delete saved search')?.click();

    expect(onOpenSaved.mock.calls.map((call) => call[1])).toEqual([
      'currentPane',
      'otherPane',
      'newTab',
    ]);
    expect(onToggleSavedPin).toHaveBeenCalledWith(saved.id);
    expect(onDeleteSaved).toHaveBeenCalledWith(saved.id);
    expect(document.querySelector('.fm-icon-star-filled')).not.toBeNull();
    expect(document.querySelectorAll('.fm-saved-search-actions button')).toHaveLength(6);
  });

  it('does not retain the edited search id after cancellation', () => {
    const onSave = vi.fn();
    const saved = {
      id: '11111111-1111-4111-8111-111111111111',
      name: 'Large videos',
      pinned: false,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Videos' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: ['video/*'],
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    };
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [saved],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
          onSave,
        }),
    });
    m.redraw.sync();
    const button = (label: string) =>
      [...document.querySelectorAll<HTMLButtonElement>('button')].find(
        (candidate) => candidate.textContent?.trim() === label,
      );
    document.querySelector<HTMLButtonElement>('[aria-label="Edit saved search"]')?.click();
    button('Cancel')?.click();

    const name = document.querySelector<HTMLInputElement>('input[placeholder="Saved search name"]');
    if (!name) throw new Error('saved search name input missing');
    name.value = 'Documents';
    name.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    button('Save search')?.click();

    expect(onSave).toHaveBeenCalledWith(
      'Documents',
      expect.objectContaining({ filenameQuery: '' }),
      undefined,
    );
  });

  it('keeps a long saved-search collection in a dedicated scroll region', () => {
    const savedSearches = Array.from({ length: 30 }, (_, index) => ({
      id: `11111111-1111-4111-8111-${index.toString().padStart(12, '0')}`,
      name: `Saved search ${index + 1}`,
      pinned: false,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Documents' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: [],
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    }));
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches,
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const list = document.querySelector<HTMLElement>('.fm-saved-search-list');
    expect(list?.children).toHaveLength(30);
    expect(list?.classList.contains('fm-saved-search-list')).toBe(true);
  });

  it('uses the same flat-button treatment for disabled search and save actions', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const save = document.querySelector<HTMLButtonElement>('.fm-save-search-button');
    const search = [...document.querySelectorAll<HTMLButtonElement>('.modal-footer button')].find(
      (button) => button.textContent?.trim() === 'Search',
    );
    expect(save?.disabled).toBe(true);
    expect(search?.disabled).toBe(true);
    expect(save?.classList.contains('btn-flat')).toBe(true);
    expect(search?.classList.contains('btn-flat')).toBe(true);
  });
});
