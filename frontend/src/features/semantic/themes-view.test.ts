import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { mockCorpusThemes } from '../../api/client/mock-corpus-themes';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { RagScope } from '../../models';
import { ThemesView } from './themes-view';

let root: HTMLElement;

const scope = (
  kind: 'entireLibrary' | 'currentFolder',
  label: string,
  folderUri = 'mock:///Documents/handbook/chapters',
): RagScope => ({
  kind,
  ...(kind === 'currentFolder' ? { folder: { providerId: 'file', uri: folderUri } } : {}),
  workspaceId: '22222222-2222-4222-8222-222222222222',
  label,
  selectedFiles: [],
  semanticSourceIds: [],
  enrolledRootIds: [],
});

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function mount(
  client: MockFileManagerClient,
  overrides: Partial<Parameters<typeof ThemesView>[0]['attrs']> = {},
): {
  onOpenPassage: ReturnType<typeof vi.fn>;
  onSearchSources: ReturnType<typeof vi.fn>;
} {
  const onOpenPassage = vi.fn();
  const onSearchSources = vi.fn();
  m.mount(root, {
    view: () =>
      m(ThemesView, {
        client,
        scopes: [scope('entireLibrary', 'Entire library')],
        onOpenPassage,
        onSearchSources,
        ...overrides,
      }),
  });
  return { onOpenPassage, onSearchSources };
}

describe('ThemesView', () => {
  it('shows a busy skeleton, then an accessible theme list with keywords and shares', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    mount(client);
    m.redraw.sync();
    expect(root.textContent).toContain('Finding themes');
    await vi.waitFor(() => expect(analyze).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-list')).not.toBeNull());

    const rows = [...root.querySelectorAll('.fm-themes-row')];
    expect(rows.map((row) => row.querySelector('.fm-themes-row-label')?.textContent)).toEqual([
      'harbour, shipping',
      'mortgage, interest',
      'violin',
      'No theme',
    ]);
    expect(rows[0]?.textContent).toContain('10 · 42%');
    expect(root.querySelector('ul[aria-label="Themes"]')).not.toBeNull();
    // The canvas is decorative; the list carries the content.
    expect(root.querySelector('canvas')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('selecting a theme reveals its keywords, passages and strongest documents', async () => {
    const client = new MockFileManagerClient();
    mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.querySelector('.fm-themes-details')).toBeNull();

    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    const details = root.querySelector('.fm-themes-details');
    expect(details?.textContent).toContain('Keywords');
    expect(details?.textContent).toContain('shipping');
    expect(details?.textContent).toContain('Documents in this theme');
    expect(details?.textContent).toContain('Port report.txt');
    expect(root.querySelector('.fm-themes-row[aria-pressed="true"]')).not.toBeNull();

    // Selecting the same theme again clears the filter.
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-themes-details')).toBeNull();
  });

  it('shows SKOS concepts only when the analysis carries them', async () => {
    const client = new MockFileManagerClient();
    mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-themes-concepts')).toBeNull();
    m.mount(root, null);

    const labelled = new MockFileManagerClient();
    const original = await labelled.analyzeCorpusThemes({ scope: scope('entireLibrary', 'x') });
    const themes = original.themes;
    if (themes === undefined || themes === null) throw new Error('mock themes expected');
    const first = themes.themes[0];
    if (first === undefined) throw new Error('mock theme expected');
    vi.spyOn(labelled, 'analyzeCorpusThemes').mockResolvedValue({
      ...original,
      themes: {
        ...themes,
        themes: [
          {
            ...first,
            concepts: [{ conceptId: 'https://example.org/concept/wind-energy', chunkCount: 4 }],
          },
          ...themes.themes.slice(1),
        ],
      },
    });
    mount(labelled);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-themes-concepts')?.textContent).toContain('wind energy (4)');
  });

  it('opens a passage source through the host and reports a failure', async () => {
    const client = new MockFileManagerClient();
    const { onOpenPassage } = mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();

    onOpenPassage.mockRejectedValueOnce(new Error('gone'));
    (root.querySelector('.fm-themes-examples .fm-themes-source') as HTMLAnchorElement).click();
    expect(onOpenPassage).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: 'mock-source-a', chunkId: 'chunk-a' }),
    );
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('.fm-themes-error')?.textContent).toBe('gone');
    });
  });

  it('flags a stale analysis when the index revision moves on and re-analyses on request', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.querySelector('.fm-themes-stale')).toBeNull();
    m.mount(root, null);
    vi.spyOn(client, 'getCorpusThemeStatus').mockResolvedValue({
      indexRevision: 'changed',
      documentCount: 3,
      chunkCount: 30,
      modelId: 'mock-embedding-model',
      profileTooWeak: false,
    });
    mount(client);
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('.fm-themes-stale')).not.toBeNull();
    });
    const before = analyze.mock.calls.length;
    (root.querySelector('.fm-themes-stale button') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(analyze.mock.calls.length).toBe(before + 1));
  });

  it('explains a profile that is too weak instead of showing an empty map', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'analyzeCorpusThemes').mockResolvedValue({
      outcome: 'profileTooWeak',
      modelId: 'procyon.dev.hashing-embedding',
      themes: null,
    });
    mount(client);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-themes-notice')?.textContent).toContain(
        'procyon.dev.hashing-embedding',
      ),
    );
    expect(root.querySelector('canvas')).toBeNull();
    expect(root.querySelector('.fm-themes-list')).toBeNull();
  });

  it('offers no folder scope when the folder is outside every enrolled root', async () => {
    const client = new MockFileManagerClient();
    mount(client, {
      scopes: [
        scope('entireLibrary', 'Entire library'),
        scope('currentFolder', 'Current folder', 'mock:///Downloads/triz'),
      ],
    });
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.querySelector('.fm-themes-scope')).toBeNull();
  });

  it('switches scope and summarizes documents, passages and displayed points', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    analyze.mockResolvedValue({
      outcome: 'ready',
      modelId: 'mock-embedding-model',
      themes: { ...mockCorpusThemes(), mapSampled: true },
    });
    mount(client, {
      scopes: [scope('entireLibrary', 'Entire library'), scope('currentFolder', 'Current folder')],
    });
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.textContent).toMatch(/\d+ documents · 24 passages · 24 displayed/);

    const buttons = [...root.querySelectorAll<HTMLButtonElement>('.fm-themes-scope')];
    expect(buttons.map((button) => button.textContent)).toEqual([
      'Entire library',
      'Current folder: chapters',
    ]);
    buttons[1]?.click();
    await vi.waitFor(() => expect(analyze).toHaveBeenCalledTimes(2));
    expect(analyze.mock.calls[1]?.[0].scope.kind).toBe('currentFolder');
  });

  it('hands the sources of the selected theme to a scoped search', async () => {
    const client = new MockFileManagerClient();
    const sources = vi.spyOn(client, 'getCorpusThemeSources');
    const { onSearchSources } = mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();

    (root.querySelector('.fm-themes-search') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(onSearchSources).toHaveBeenCalledOnce());
    expect(sources).toHaveBeenCalledWith(
      expect.objectContaining({ theme: 0, fingerprint: 'mock-fingerprint' }),
    );
    expect(onSearchSources).toHaveBeenCalledWith(
      ['mock-source-a', 'mock-source-b'],
      'harbour, shipping ',
    );
  });

  it('lists only documents above the share threshold, as links that open the source', async () => {
    const client = new MockFileManagerClient();
    const { onOpenPassage } = mount(client, { minDocumentShare: 0.99 });
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-themes-document-list')).toBeNull();

    m.mount(root, null);
    const opened = mount(client, { minDocumentShare: 0 });
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('ol.fm-themes-document-list')).not.toBeNull();
    (root.querySelector('.fm-themes-document-list a') as HTMLAnchorElement).click();
    expect(opened.onOpenPassage).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.any(String) }),
    );
    expect(onOpenPassage).not.toHaveBeenCalled();
  });

  it('re-analyses instead of searching when the themes went stale', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    const { onSearchSources } = mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();

    vi.spyOn(client, 'getCorpusThemeSources').mockRejectedValue(
      Object.assign(new Error('stale'), { details: { reason: 'corpusThemesStale' } }),
    );
    (root.querySelector('.fm-themes-search') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(analyze).toHaveBeenCalledTimes(2));
    expect(onSearchSources).not.toHaveBeenCalled();
  });

  it('shows other failures of the search preparation', async () => {
    const client = new MockFileManagerClient();
    const { onSearchSources } = mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    vi.spyOn(client, 'getCorpusThemeSources').mockRejectedValue(new Error('denied'));
    (root.querySelector('.fm-themes-search') as HTMLButtonElement).click();
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('.fm-themes-error')?.textContent).toBe('denied');
    });
    expect(onSearchSources).not.toHaveBeenCalled();
  });

  describe('folder scope', () => {
    const HANDBOOK = 'mock-knowledge-root-handbook';
    const ARCHIVE = 'mock-knowledge-root-archive';

    const checkboxes = (): HTMLInputElement[] => [
      ...root.querySelectorAll<HTMLInputElement>('.fm-themes-root input[type="checkbox"]'),
    ];
    const applyButton = (): HTMLButtonElement | undefined =>
      [...root.querySelectorAll<HTMLButtonElement>('.fm-themes-root-actions button')].find(
        (button) => button.textContent === 'Show themes for these folders',
      );

    it('analyses every folder at first and offers no panel with fewer than two roots', async () => {
      const client = new MockFileManagerClient();
      const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
      mount(client);
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(analyze.mock.calls[0]?.[0].scope.kind).toBe('entireLibrary');
      expect(checkboxes().map((box) => box.checked)).toEqual([true, true]);
      expect(root.querySelector('.fm-themes-root')?.textContent).toContain(
        'Handbook · 3 documents',
      );
      m.mount(root, null);

      const single = new MockFileManagerClient();
      vi.spyOn(single, 'listKnowledgeRoots').mockResolvedValue([
        (await new MockFileManagerClient().listKnowledgeRoots({ workspaceId: 'w' }))[0] as never,
      ]);
      mount(single);
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(root.querySelector('.fm-themes-roots')).toBeNull();
    });

    it('analyses only the chosen folders and remembers the choice', async () => {
      const client = new MockFileManagerClient();
      const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
      const onSelectRoots = vi.fn();
      mount(client, { onSelectRoots });
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(applyButton()?.disabled).toBe(true);

      checkboxes()[1]?.click();
      m.redraw.sync();
      expect(applyButton()?.disabled).toBe(false);
      expect(analyze).toHaveBeenCalledOnce();
      applyButton()?.click();

      await vi.waitFor(() => expect(analyze).toHaveBeenCalledTimes(2));
      const scope = analyze.mock.calls[1]?.[0].scope;
      expect(scope?.kind).toBe('enrolledRoots');
      expect(scope?.enrolledRootIds).toEqual([HANDBOOK]);
      expect(onSelectRoots).toHaveBeenCalledWith([HANDBOOK]);
    });

    it('stores an empty list when every folder is chosen again', async () => {
      const client = new MockFileManagerClient();
      const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
      const onSelectRoots = vi.fn();
      mount(client, { onSelectRoots, selectedRootIds: [ARCHIVE] });
      await vi.waitFor(() => expect(analyze).toHaveBeenCalledOnce());
      expect(analyze.mock.calls[0]?.[0].scope.enrolledRootIds).toEqual([ARCHIVE]);
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(checkboxes().map((box) => box.checked)).toEqual([false, true]);

      checkboxes()[0]?.click();
      m.redraw.sync();
      applyButton()?.click();
      await vi.waitFor(() => expect(analyze).toHaveBeenCalledTimes(2));
      expect(analyze.mock.calls[1]?.[0].scope.kind).toBe('entireLibrary');
      expect(onSelectRoots).toHaveBeenCalledWith([]);
    });

    it('ignores remembered folders that no longer exist and never applies an empty choice', async () => {
      const client = new MockFileManagerClient();
      const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
      mount(client, { selectedRootIds: ['removed-root'] });
      await vi.waitFor(() => expect(analyze).toHaveBeenCalledOnce());
      expect(analyze.mock.calls[0]?.[0].scope.kind).toBe('entireLibrary');
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());

      for (const box of checkboxes()) box.click();
      m.redraw.sync();
      expect(applyButton()?.disabled).toBe(true);
    });
  });

  describe('export', () => {
    const FOLDER = { providerId: 'file', uri: 'mock:///Documents/out/' };

    const panel = (): HTMLElement | null => root.querySelector('.fm-themes-export');
    const field = <T extends HTMLElement>(selector: string): T => {
      const found = root.querySelector<T>(`.fm-themes-export ${selector}`);
      if (found === null) throw new Error(`missing ${selector}`);
      return found;
    };
    const exportButton = (): HTMLButtonElement =>
      [...root.querySelectorAll<HTMLButtonElement>('.fm-themes-export button')].find(
        (button) => button.textContent === 'Save in the current folder',
      ) as HTMLButtonElement;

    it('is offered only when a destination folder is known', async () => {
      const client = new MockFileManagerClient();
      mount(client);
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(panel()).toBeNull();
    });

    it('exports all themes as Markdown by default without document text', async () => {
      const client = new MockFileManagerClient();
      const exportThemes = vi.spyOn(client, 'exportCorpusThemes');
      const onExported = vi.fn();
      mount(client, { exportFolder: FOLDER, onExported });
      await vi.waitFor(() => expect(panel()).not.toBeNull());

      exportButton().click();
      await vi.waitFor(() => expect(exportThemes).toHaveBeenCalledOnce());
      const request = exportThemes.mock.calls[0]?.[0];
      expect(request?.format).toBe('markdown');
      expect(request?.destination.uri).toMatch(
        /^mock:\/\/\/Documents\/out\/corpus-themes-\d{4}-\d{2}-\d{2}\.md$/u,
      );
      expect(request?.overwrite).toBe(false);
      expect(request?.themes).toEqual([]);
      expect(request?.includePassages).toBe(false);
      expect(request?.includeDocuments).toBe(false);
      expect(request?.baseUri).toBeUndefined();
      expect(request?.labels?.[0]?.label).toBe('harbour, shipping');
      await vi.waitFor(() => expect(root.textContent).toContain('themes) in the current folder.'));
      expect(onExported).toHaveBeenCalledOnce();
    });

    it('sends the chosen format, opt-ins and base URI for SKOS, and only the selected theme', async () => {
      const client = new MockFileManagerClient();
      const exportThemes = vi.spyOn(client, 'exportCorpusThemes');
      mount(client, { exportFolder: FOLDER });
      await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
      expect(root.querySelector('.fm-themes-export input[type="url"]')).toBeNull();

      const select = field<HTMLSelectElement>('select');
      select.value = 'skosTurtle';
      select.dispatchEvent(new Event('change'));
      m.redraw.sync();
      const [passages, documents] = [
        ...root.querySelectorAll<HTMLInputElement>('.fm-themes-export input[type="checkbox"]'),
      ];
      passages?.click();
      documents?.click();
      const base = field<HTMLInputElement>('input[type="url"]');
      base.value = ' https://example.org/themes ';
      base.dispatchEvent(new Event('input'));

      const radios = [...root.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
      expect(radios[1]?.disabled).toBe(true);
      (root.querySelectorAll('.fm-themes-row')[1] as HTMLButtonElement).click();
      m.redraw.sync();
      expect(radios[1]?.disabled).toBe(false);
      radios[1]?.click();
      m.redraw.sync();

      exportButton().click();
      await vi.waitFor(() => expect(exportThemes).toHaveBeenCalledOnce());
      const request = exportThemes.mock.calls[0]?.[0];
      expect(request?.format).toBe('skosTurtle');
      expect(request?.destination.uri).toMatch(/corpus-theme-2-\d{4}-\d{2}-\d{2}\.ttl$/u);
      expect(request?.themes).toEqual([1]);
      expect(request?.includePassages).toBe(true);
      expect(request?.includeDocuments).toBe(true);
      expect(request?.baseUri).toBe('https://example.org/themes');
    });

    it('reports a failed export without claiming success', async () => {
      const client = new MockFileManagerClient();
      vi.spyOn(client, 'exportCorpusThemes').mockRejectedValue(new Error('File already exists'));
      const onExported = vi.fn();
      mount(client, { exportFolder: FOLDER, onExported });
      await vi.waitFor(() => expect(panel()).not.toBeNull());
      exportButton().click();
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-themes-export .fm-themes-error')).not.toBeNull(),
      );
      expect(root.querySelector('.fm-themes-export .fm-themes-error')?.textContent).toBe(
        'File already exists',
      );
      expect(onExported).not.toHaveBeenCalled();
    });
  });
});
