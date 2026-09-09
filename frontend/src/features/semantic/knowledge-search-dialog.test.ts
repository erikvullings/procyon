import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { KnowledgeAnswer, KnowledgeEvidence } from '../../models';
import {
  groupEvidenceByDocument,
  KnowledgeSearchDialog,
  knowledgeProvenanceLabel,
} from './knowledge-search-dialog';

let root: HTMLElement;

const workspaceId = '22222222-2222-4222-8222-222222222222';
const preferencesStorageKey = 'procyon.knowledgeSearch.preferences.v1';

interface MountOptions {
  readonly client?: MockFileManagerClient;
  readonly currentFolder?: { providerId: string; uri: string };
  readonly semanticSourceIds?: readonly string[];
  readonly initialSubject?: string;
  readonly onClose?: () => void;
  readonly onOpenSource?: (evidence: KnowledgeEvidence) => void | Promise<void>;
}

function mount(options: MountOptions = {}): MockFileManagerClient {
  const client = options.client ?? new MockFileManagerClient();
  m.mount(root, {
    view: () =>
      m(KnowledgeSearchDialog, {
        open: true,
        client,
        workspaceId,
        currentFolder: options.currentFolder,
        semanticSourceIds: options.semanticSourceIds ?? [],
        initialSubject: options.initialSubject,
        onClose: options.onClose ?? vi.fn(),
        ...(options.onOpenSource === undefined ? {} : { onOpenSource: options.onOpenSource }),
      }),
  });
  m.redraw.sync();
  return client;
}

function subjects(): HTMLTextAreaElement {
  const element = root.querySelector<HTMLTextAreaElement>('#fm-knowledge-subjects');
  if (element === null) throw new Error('subject input not rendered');
  return element;
}

function dsl(): HTMLTextAreaElement {
  const element = root.querySelector<HTMLTextAreaElement>('#fm-knowledge-dsl-input');
  if (element === null) throw new Error('DSL input not rendered');
  return element;
}

function type(element: HTMLTextAreaElement | HTMLInputElement, value: string): void {
  element.value = value;
  element.dispatchEvent(new InputEvent('input', { bubbles: true }));
  m.redraw.sync();
}

function button(label: string): HTMLButtonElement {
  const found = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) =>
      candidate.textContent?.trim() === label || candidate.getAttribute('aria-label') === label,
  );
  if (found === undefined) throw new Error(`button not rendered: ${label}`);
  return found;
}

/** Waits until the dialog finished loading its reported capabilities and roots. */
async function ready(): Promise<void> {
  await vi.waitFor(() => {
    expect(root.textContent).not.toContain('Loading knowledge search…');
    expect(root.querySelector('.fm-knowledge-advanced')).not.toBeNull();
  });
  m.redraw.sync();
}

async function search(): Promise<void> {
  submitSearch();
  await vi.waitFor(() =>
    expect(root.querySelector('.fm-knowledge-results-heading[role="status"]')).not.toBeNull(),
  );
  m.redraw.sync();
}

function submitSearch(): void {
  subjects().dispatchEvent(
    new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
  );
}

beforeEach(() => {
  setLocale('en');
  localStorage.removeItem(preferencesStorageKey);
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.restoreAllMocks();
});

describe('KnowledgeSearchDialog (task 0206)', () => {
  it('uses a fixed filter-style toolbar and moves search settings into a modal', async () => {
    mount();

    await ready();
    const toolbar = root.querySelector('.fm-knowledge-search-toolbar');
    expect(toolbar).not.toBeNull();
    expect(toolbar?.querySelector('#fm-knowledge-subjects')).not.toBeNull();
    const submit = toolbar?.querySelector<HTMLButtonElement>(
      'button.fm-knowledge-search-submit[aria-label="Search"]',
    );
    expect(submit?.classList.contains('btn-icon')).toBe(true);
    expect(submit?.querySelector('.fm-icon-corner-down-left')).not.toBeNull();
    expect(submit?.textContent).toBe('');
    const settings = toolbar?.querySelector<HTMLButtonElement>(
      'button[aria-label="Search settings"]',
    );
    expect(settings).not.toBeNull();
    expect(root.querySelector('.fm-knowledge-composer > .fm-knowledge-needs')).toBeNull();
    expect(root.querySelector('.fm-knowledge-composer > .fm-knowledge-advanced')).toBeNull();

    settings?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-settings-modal')).not.toBeNull();
    expect(root.querySelector('.fm-knowledge-settings-modal .fm-knowledge-needs')).not.toBeNull();
    expect(
      root.querySelector('.fm-knowledge-settings-modal .fm-knowledge-advanced'),
    ).not.toBeNull();
  });

  it('restores and persists selected knowledge needs across pane instances', async () => {
    localStorage.setItem(
      preferencesStorageKey,
      JSON.stringify({ needs: ['definition', 'procedure', 'not-a-need'] }),
    );
    mount();
    await ready();

    const inputs = [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')];
    expect(
      inputs.filter((input) => input.checked).map((input) => input.parentElement?.textContent),
    ).toEqual(['Definition', 'Procedure']);

    inputs[3]?.click();
    expect(JSON.parse(localStorage.getItem(preferencesStorageKey) ?? '{}')).toEqual({
      needs: ['definition', 'procedure', 'examples'],
    });

    m.mount(root, null);
    mount();
    await ready();
    expect(
      [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')]
        .filter((input) => input.checked)
        .map((input) => input.parentElement?.textContent),
    ).toEqual(['Definition', 'Procedure', 'Examples']);
  });

  it('keeps safe defaults when preference storage is malformed or unavailable', async () => {
    localStorage.setItem(preferencesStorageKey, '{not-json');
    mount();
    await ready();
    expect(
      [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')].some(
        (input) => input.checked,
      ),
    ).toBe(false);

    m.mount(root, null);
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('storage unavailable');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('storage unavailable');
    });
    mount();
    await ready();
    expect(() =>
      root.querySelector<HTMLInputElement>('.fm-knowledge-needs input')?.click(),
    ).not.toThrow();
  });

  it('focuses the subject field and shows the idle state before any search', async () => {
    mount();

    await ready();
    expect(document.activeElement).toBe(subjects());
    expect(root.textContent).toContain('Compose a subject, then search your indexed documents.');
    expect(root.querySelector('.fm-knowledge-results')).toBeNull();
  });

  it('keeps answer-generation copy out of the common flow when it is unavailable', async () => {
    mount({ initialSubject: 'retrieval' });

    await ready();
    expect(root.textContent).not.toContain('Search never generates an answer');
    expect(root.textContent).not.toContain('Generate answer');
    expect(root.textContent).not.toContain('Your generated answer will appear here.');
    expect(root.querySelector('.fm-rag-answer')).toBeNull();
  });

  it('offers every knowledge need and every scope', async () => {
    mount();

    await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-needs')).not.toBeNull());
    const needs = [...root.querySelectorAll('.fm-knowledge-needs label span')].map(
      (label) => label.textContent,
    );
    expect(needs).toEqual([
      'Overview',
      'Definition',
      'Procedure',
      'Examples',
      'Evidence',
      'Arguments',
      'Comparison',
      'Limitations',
      'References',
    ]);
    const scope = root.querySelector<HTMLSelectElement>('#fm-knowledge-scope');
    expect([...(scope?.options ?? [])].map((option) => option.text)).toEqual([
      'Entire authorized library',
      'Selected indexed roots',
      'Current indexed folder',
      'Current semantic result set',
    ]);
  });

  it('keeps the compact composer and the advanced DSL equivalent in sync', async () => {
    mount();
    await ready();

    type(subjects(), 'hybrid retrieval');
    await vi.waitFor(() => expect(dsl().value).toContain('about: "hybrid retrieval"'));

    root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')[1]?.click();
    await vi.waitFor(() => expect(dsl().value).toContain('need: definition'));

    type(dsl(), 'about: fusion\nneed: limitations\nrelated: bm25');
    await vi.waitFor(() => expect(subjects().value).toBe('fusion'));
    expect(root.querySelector('#fm-knowledge-related')).toBeNull();
    expect(dsl().value).toContain('related: bm25');
    const checked = [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')]
      .map((input, index) => (input.checked ? index : undefined))
      .filter((index) => index !== undefined);
    expect(checked).toEqual([7]);
  });

  it('keeps parser diagnostics with the advanced query language', async () => {
    mount();
    await ready();

    type(dsl(), 'about: retrieval nonsense: x');

    await vi.waitFor(() => expect(root.textContent).toContain('Unknown field "nonsense"'));
    expect(root.querySelector('.fm-knowledge-diagnostics [role="alert"]')).not.toBeNull();
    expect(root.textContent).not.toContain('Interpretation');
    expect(root.textContent).not.toContain('Read as explicit query fields.');
  });

  it('does not expose parser interpretation mechanics in the common flow', async () => {
    mount();
    await ready();

    type(dsl(), 'compare hybrid and dense retrieval');

    await vi.waitFor(() => expect(subjects().value).toBe('"hybrid and dense retrieval"'));
    expect(root.textContent).not.toContain('Alternative readings');
    expect(root.textContent).not.toContain('More than one reading was possible.');
  });

  it('lists answer-only fields explicitly excluded from retrieval', async () => {
    mount();
    await ready();

    type(dsl(), 'about: retrieval do: apply to: "write a scheduler" format: steps');

    await vi.waitFor(() => expect(button('Query plan').disabled).toBe(false));
    button('Query plan').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Not used for retrieval'));
    const excluded = root.querySelector('.fm-knowledge-excluded');
    expect(excluded?.textContent).toContain('do: apply');
    expect(excluded?.textContent).toContain('to: write a scheduler');
    expect(excluded?.textContent).toContain('format: steps');
  });

  it('previews the deterministic plan with reasons and priorities', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')[1]?.click();
    m.redraw.sync();

    button('Query plan').click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-planned-searches')).not.toBeNull(),
    );
    const searches = root.querySelector('.fm-knowledge-planned-searches');
    expect(searches?.textContent).toContain('retrieval');
    expect(searches?.textContent).toContain('retrieval definition');
    expect(searches?.textContent).toContain('subject');
    expect(searches?.textContent).toContain('Need: Definition');
    expect(root.textContent).toContain('Planner structured-knowledge-planner/1');
    expect(root.textContent).toContain('Entire indexed library');
  });

  it('runs a complete search with no language model as ranked documents', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')[1]?.click();
    m.redraw.sync();

    await search();

    expect(root.querySelector('.fm-knowledge-results-section')?.getAttribute('aria-label')).toBe(
      'Knowledge search results',
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
    expect(root.querySelector('.fm-knowledge-document-list')).not.toBeNull();
    expect(root.querySelector('.fm-knowledge-document-item h4')).not.toBeNull();
    expect(root.textContent).toContain('Retrieval design notes');
    expect(root.querySelector('.fm-knowledge-results-heading')?.textContent).toMatch(
      /^Sources: \d+ document\(s\) · \d+ matching section\(s\)$/u,
    );
    expect(root.querySelector('.fm-knowledge-result-summary')).toBeNull();
    expect(root.querySelector('.fm-knowledge-results-section')?.textContent).not.toContain(
      'Subject',
    );
  });

  it('renders decoded document-ranked Markdown without per-chunk query diagnostics', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) return result;
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'TRIZ%20Substance-Field%20Modelling.pdf',
            content: '## Su-Field model\n\nA **substance-field** section.',
            sectionPath: ['Standards', 'Su-Field synthesis'],
            provenance:
              '{"kind":"exact","value":{"kind":"pdfBlock","page_number":167,"block_index":3}}',
            unavailable: false,
          },
          {
            ...first,
            recordId: `${first.recordId}-page-168`,
            title: 'TRIZ%20Substance-Field%20Modelling.pdf',
            content: 'A continuation without an indexed heading.',
            sectionPath: [],
            provenance:
              '{"kind":"span","value":{"first":{"kind":"pdfBlock","page_number":168,"block_index":0},"last":{"kind":"pdfBlock","page_number":168,"block_index":2}}}',
            sourcePosition: first.sourcePosition + 1,
            unavailable: false,
          },
        ],
      };
    });
    const onOpenSource = vi.fn();
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await ready();

    await search();

    expect(root.querySelectorAll('.fm-knowledge-document-item')).toHaveLength(1);
    expect(root.querySelector('.fm-knowledge-document-number')?.textContent).toBe('1.');
    expect(root.querySelector('.fm-knowledge-source-link span')?.textContent).toBe(
      'TRIZ Substance-Field Modelling.pdf',
    );
    expect(root.querySelector('.fm-knowledge-section-title')?.textContent).toBe(
      'Standards / Su-Field synthesis',
    );
    const pages = root.querySelectorAll<HTMLButtonElement>('.fm-knowledge-page-link');
    expect(pages[0]?.textContent).toBe('Page 167');
    expect(pages[1]?.textContent).toBe('Page 168');
    expect(root.textContent).not.toContain('Matching section');
    expect(root.querySelector('.fm-knowledge-result-markdown h2')?.textContent).toBe(
      'Su-Field model',
    );
    expect(root.querySelector('.fm-knowledge-result-markdown strong')?.textContent).toBe(
      'substance-field',
    );
    expect(root.querySelector('.fm-knowledge-reasons')).toBeNull();
    expect(root.querySelector('.fm-knowledge-result-meta')).toBeNull();
    expect(root.querySelector('#fm-knowledge-grouping')).toBeNull();

    pages[0]?.click();
    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({
        sectionPath: ['Standards', 'Su-Field synthesis'],
        provenance: expect.stringContaining('"page_number":167'),
      }),
    );
  });

  it('explains that a hybrid request ran as full text when embeddings are unavailable', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    expect(root.textContent).toContain('runs as full text only');

    await search();

    expect(root.querySelector('.fm-knowledge-result-details')?.textContent).toContain(
      'Requested Hybrid, ran Full text: query embeddings are unavailable',
    );
  });

  it('disables the semantic-only mode when query embeddings are unavailable', async () => {
    mount();
    await ready();

    const semantic = [...root.querySelectorAll<HTMLButtonElement>('.fm-knowledge-mode')].find(
      (candidate) => candidate.textContent === 'Semantic',
    );
    expect(semantic?.disabled).toBe(true);
    expect(semantic?.getAttribute('title')).toContain('query embeddings');
    const fullText = [...root.querySelectorAll<HTMLButtonElement>('.fm-knowledge-mode')].find(
      (candidate) => candidate.textContent === 'Full text',
    );
    expect(fullText?.disabled).toBe(false);
  });

  it('shows freshness, availability and coverage honestly', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(root.textContent).toContain('Source changed since indexing');
    expect(root.querySelector('.fm-knowledge-result-details')?.textContent).toContain('Coverage:');
  });

  it('opens the exact source of one evidence row', async () => {
    const onOpenSource = vi.fn();
    mount({ initialSubject: 'retrieval', onOpenSource });
    await ready();
    await search();

    const link = root.querySelector<HTMLButtonElement>('.fm-knowledge-source-link:not(:disabled)');
    link?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.stringContaining('mock-knowledge-source-') }),
    );
  });

  it('reports a failure to open a source without losing the results', async () => {
    mount({
      initialSubject: 'retrieval',
      onOpenSource: () => Promise.reject(new Error('nope')),
    });
    await ready();
    await search();

    root.querySelector<HTMLButtonElement>('.fm-knowledge-source-link:not(:disabled)')?.click();

    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toContain(
        'The source could not be opened.',
      ),
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('reports an empty result set rather than an error', async () => {
    mount({ initialSubject: 'zzzznotindexedzzzz' });
    await ready();

    await search();

    expect(root.textContent).toContain('No indexed source matched this query.');
    expect(root.querySelector('[role="alert"]')).toBeNull();
  });

  it('surfaces a search failure as an alert', async () => {
    const client = new MockFileManagerClient({
      failures: { executeKnowledgeSearch: new Error('boom') },
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    submitSearch();

    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toBe(
        'Knowledge search encountered an internal error. Try again; if it continues, rebuild the semantic index.',
      ),
    );
  });

  it('gives recovery guidance when the selected scope has no indexed sources', async () => {
    const failure = Object.assign(new Error('resource not found'), { code: 'notFound' });
    const client = new MockFileManagerClient({
      failures: { executeKnowledgeSearch: failure },
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    submitSearch();

    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toBe(
        'No indexed documents are available in this scope. Choose another scope or index a folder, then try again.',
      ),
    );
  });

  it('reports an unavailable knowledge capability when loading fails', async () => {
    const client = new MockFileManagerClient({
      failures: { getKnowledgeCapabilities: new Error('offline') },
    });
    mount({ client });

    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toBe(
        'Knowledge search is not available right now.',
      ),
    );
  });

  it('warns when the backend event stream is closed', async () => {
    mount();

    await vi.waitFor(() => expect(root.textContent).toContain('The backend is unreachable'));
  });

  it('shows only a discrete status spinner while searching', async () => {
    const client = new MockFileManagerClient({ latencyMs: 50 });
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    submitSearch();
    m.redraw.sync();
    expect(root.querySelector('.fm-knowledge-results-section')?.getAttribute('aria-busy')).toBe(
      'true',
    );
    expect(root.querySelector('.fm-knowledge-search-spinner')).not.toBeNull();
    expect(button('Search').disabled).toBe(true);
    expect(() => button('Cancel search')).toThrow();

    await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-search-spinner')).toBeNull());
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('closes on Escape from the subject field and aborts the running search', async () => {
    const onClose = vi.fn();
    const client = new MockFileManagerClient({ latencyMs: 50 });
    mount({ client, initialSubject: 'retrieval', onClose });
    await ready();
    submitSearch();
    m.redraw.sync();

    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(onClose).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(root.textContent).toContain('The search was cancelled.'));
  });

  it('searches with Enter and preserves Shift+Enter for a newline', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    const newline = new KeyboardEvent('keydown', {
      key: 'Enter',
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    subjects().dispatchEvent(newline);
    expect(newline.defaultPrevented).toBe(false);
    expect(execute).not.toHaveBeenCalled();
    expect(button('Search').disabled).toBe(false);

    submitSearch();

    await vi.waitFor(() => expect(execute).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(button('Search').disabled).toBe(false));
    button('Search').click();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(2));
  });

  it('labels every control and announces results politely', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    expect(subjects().getAttribute('aria-describedby')).toBeNull();
    expect(root.querySelector('#fm-knowledge-subjects-hint')).toBeNull();
    expect(root.querySelector('.fm-knowledge-needs legend')?.textContent).toBe('What do you need?');
    expect(root.querySelector('.fm-knowledge-modes legend')?.textContent).toBe('Retrieval');
    expect(root.querySelector('.fm-knowledge-results-body')?.getAttribute('aria-live')).toBe(
      'polite',
    );
    for (const mode of root.querySelectorAll('.fm-knowledge-mode')) {
      expect(mode.getAttribute('aria-pressed')).toMatch(/true|false/u);
    }
  });

  it('defaults the scope to the current semantic result set when no indexed folder is active', async () => {
    mount({ semanticSourceIds: ['mock-knowledge-source-onboarding'] });
    await ready();

    expect(root.querySelector<HTMLSelectElement>('#fm-knowledge-scope')?.value).toBe(
      'semanticResults',
    );
  });

  it('falls back to the entire authorized library with no other scope', async () => {
    mount();
    await ready();

    expect(root.querySelector<HTMLSelectElement>('#fm-knowledge-scope')?.value).toBe(
      'entireLibrary',
    );
  });

  it('lists selectable indexed roots and marks the unavailable one', async () => {
    mount();
    await ready();
    const scope = root.querySelector<HTMLSelectElement>('#fm-knowledge-scope');
    if (scope === null) throw new Error('scope select not rendered');

    scope.value = 'enrolledRoots';
    scope.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();

    const labels = [...root.querySelectorAll('.fm-knowledge-roots label span')].map(
      (label) => label.textContent,
    );
    expect(labels).toEqual(['Handbook', 'Archive · unavailable']);
  });

  it('always groups results by document', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    await search();
    const documentGroups = [...root.querySelectorAll('.fm-knowledge-group')].map(
      (group) => group.querySelector('.fm-knowledge-source-link')?.textContent,
    );
    expect(documentGroups).toContain('Retrieval design notes');
    expect(root.querySelector('#fm-knowledge-grouping')).toBeNull();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });
  it('keeps half-typed DSL text intact while it is being edited', async () => {
    mount();
    await ready();

    type(dsl(), 'about: retrieval\nneed: d');

    await vi.waitFor(() => expect(root.textContent).toContain('Unknown need "d"'));
    expect(dsl().value).toBe('about: retrieval\nneed: d');
  });

  it('preserves a scope selector typed in the DSL instead of dropping it', async () => {
    mount();
    await ready();

    type(dsl(), 'about: retrieval scope: root:mock-knowledge-root-handbook');
    await vi.waitFor(() => expect(subjects().value).toBe('retrieval'));

    // A composer edit re-serialises the query; the scope must survive it.
    type(subjects(), 'retrieval\nfusion');

    await vi.waitFor(() =>
      expect(dsl().value).toContain('scope: root:mock-knowledge-root-handbook'),
    );
  });

  it('does not let the plan preview clear a running search', async () => {
    const client = new MockFileManagerClient({ latencyMs: 50 });
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    submitSearch();
    m.redraw.sync();

    const advanced = root.querySelector<HTMLDetailsElement>('.fm-knowledge-plan');
    advanced?.dispatchEvent(new Event('toggle', { bubbles: true }));
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-results-section')?.getAttribute('aria-busy')).toBe(
      'true',
    );
    expect(root.querySelector('.fm-knowledge-search-spinner')).not.toBeNull();
    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.waitFor(() => expect(root.textContent).toContain('The search was cancelled.'));
  });

  it('starts a reopened dialog from a clean composer', async () => {
    const client = new MockFileManagerClient();
    let open = true;
    m.mount(root, {
      view: () =>
        m(KnowledgeSearchDialog, {
          open,
          client,
          workspaceId,
          currentFolder: undefined,
          semanticSourceIds: [],
          initialSubject: 'retrieval',
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();
    await ready();
    type(dsl(), 'about: retrieval do: apply');
    await vi.waitFor(() => expect(dsl().value).toContain('do: apply'));

    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();
    await ready();

    expect(subjects().value).toBe('retrieval');
    expect(root.textContent).not.toContain('do: apply');
  });

  it('ranks documents by their best match and sections by source position', () => {
    const row = (
      documentId: string,
      recordId: string,
      sourcePosition: number,
      finalRank: number,
      unavailable = false,
    ): KnowledgeEvidence => ({
      recordId,
      documentId,
      sourceId: `source-${documentId}`,
      duplicateSourceIds: [],
      title: 'README.md',
      excerpt: 'excerpt',
      content: 'content',
      sectionPath: [],
      provenance: '',
      chunkKind: 'chunk',
      sourcePosition,
      adjacent: false,
      generated: false,
      stale: false,
      unavailable,
      mediaType: 'text/markdown',
      modifiedAtMs: 0,
      tokenCount: 1,
      fusedScore: 0.5,
      finalRank,
      matchedSearchIndexes: [0],
      rankContributions: [],
      reasons: [],
    });

    const groups = groupEvidenceByDocument([
      row('document-b', 'record-b', 0, 2),
      row('document-a', 'record-a-later', 8, 1, true),
      row('document-a', 'record-a-earlier', 3, 4),
    ]);

    expect(groups).toHaveLength(2);
    expect(groups.map((group) => group.documentId)).toEqual(['document-a', 'document-b']);
    expect(groups[0]?.rows.map((entry) => entry.sourcePosition)).toEqual([3, 8]);
    expect(groups[0]?.bestRank).toBe(1);
    expect(groups[0]?.openEvidence.recordId).toBe('record-a-earlier');
  });

  it('reports an unknown indexed count instead of claiming zero indexed sources', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      return { ...result, coverage: { ...result.coverage, indexed: null } };
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(root.querySelector('.fm-knowledge-result-details')?.textContent).toContain(
      'indexed count unknown',
    );
    expect(root.querySelector('.fm-knowledge-results-heading')?.textContent).not.toContain(
      'Coverage: 0/',
    );
  });
});

/** Mounts the dialog through a real closed -> open lifecycle. */
function mountLifecycle(options: MountOptions = {}): {
  readonly client: MockFileManagerClient;
  open: (value: boolean) => void;
} {
  const client = options.client ?? new MockFileManagerClient();
  let isOpen = false;
  m.mount(root, {
    view: () =>
      m(KnowledgeSearchDialog, {
        open: isOpen,
        client,
        workspaceId,
        currentFolder: options.currentFolder,
        semanticSourceIds: options.semanticSourceIds ?? [],
        initialSubject: options.initialSubject,
        onClose: options.onClose ?? vi.fn(),
        ...(options.onOpenSource === undefined ? {} : { onOpenSource: options.onOpenSource }),
      }),
  });
  m.redraw.sync();
  return {
    client,
    open: (value: boolean) => {
      isOpen = value;
      m.redraw.sync();
      m.redraw.sync();
    },
  };
}

/** A promise whose settlement the test controls. */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function scopeSelect(): HTMLSelectElement {
  const element = root.querySelector<HTMLSelectElement>('#fm-knowledge-scope');
  if (element === null) throw new Error('scope select not rendered');
  return element;
}

describe('KnowledgeSearchDialog canonical query state (task 0206)', () => {
  it('keeps a quoted subject as one element through both editors', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();

    type(dsl(), 'about: "ACME, Inc.", fusion need: overview');
    await vi.waitFor(() => expect(subjects().value).toBe('"ACME, Inc."\nfusion'));

    // A visual edit re-serialises the query; the quoted subject must survive.
    root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')[1]?.click();
    await vi.waitFor(() => expect(dsl().value).toContain('about: "ACME, Inc.", fusion'));

    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalled());
    expect(execute.mock.calls[0]?.[0]?.draft.about).toEqual(['ACME, Inc.', 'fusion']);
  });

  it('accepts a quoted subject typed straight into the subject field', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();

    type(subjects(), '"ACME, Inc."\nfusion');

    await vi.waitFor(() => expect(dsl().value).toContain('about: "ACME, Inc.", fusion'));
    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalled());
    expect(execute.mock.calls[0]?.[0]?.draft.about).toEqual(['ACME, Inc.', 'fusion']);
  });

  it('keeps a related term containing a comma as one element', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    type(dsl(), 'about: retrieval related: "bm25, dense", fusion');
    await vi.waitFor(() => expect(dsl().value).toContain('related: "bm25, dense", fusion'));

    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalled());
    expect(execute.mock.calls[0]?.[0]?.draft.related).toEqual(['bm25, dense', 'fusion']);
  });

  it('clears the visual composer when the query language box is emptied', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    await vi.waitFor(() => expect(dsl().value).toContain('about: retrieval'));

    type(dsl(), '');

    await vi.waitFor(() => expect(subjects().value).toBe(''));
    expect(button('Search').disabled).toBe(true);
  });

  it('discards a parse that lands after a further edit', async () => {
    const client = new MockFileManagerClient();
    const pending = deferred<Awaited<ReturnType<typeof client.parseKnowledgeQuery>>>();
    const real = client.parseKnowledgeQuery.bind(client);
    const stale = await real({ text: 'about: stale' });
    mount({ client });
    await ready();
    vi.spyOn(client, 'parseKnowledgeQuery').mockImplementationOnce(() => pending.promise);

    type(dsl(), 'about: stale');
    type(dsl(), 'about: fresh');
    await vi.waitFor(() => expect(subjects().value).toBe('fresh'));
    pending.resolve(stale);
    await Promise.resolve();
    m.redraw.sync();

    expect(subjects().value).toBe('fresh');
  });

  it('discards a search result that lands after a further edit', async () => {
    const client = new MockFileManagerClient();
    const pending = deferred<Awaited<ReturnType<typeof client.executeKnowledgeSearch>>>();
    const real = client.executeKnowledgeSearch.bind(client);
    const stale = await real({
      requestId: 'stale',
      draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
      scope: {
        workspaceId,
        kind: 'entireLibrary',
        folder: null,
        enrolledRootIds: [],
        semanticSourceIds: [],
      },
      mode: 'fullText',
      options: null,
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(() => pending.promise);

    submitSearch();
    m.redraw.sync();
    type(subjects(), 'fusion');
    pending.resolve(stale);
    await vi.waitFor(() => expect(subjects().disabled).toBe(false));

    expect(root.querySelector('.fm-knowledge-results-heading[role="status"]')).toBeNull();
    expect(root.querySelectorAll('.fm-knowledge-result')).toHaveLength(0);
  });

  it('discards a parse that lands after the dialog was closed and reopened', async () => {
    const client = new MockFileManagerClient();
    const pending = deferred<Awaited<ReturnType<typeof client.parseKnowledgeQuery>>>();
    const real = client.parseKnowledgeQuery.bind(client);
    const stale = await real({ text: 'about: leftover' });
    const lifecycle = mountLifecycle({ client });
    lifecycle.open(true);
    await ready();
    vi.spyOn(client, 'parseKnowledgeQuery').mockImplementationOnce(() => pending.promise);

    type(dsl(), 'about: leftover');
    lifecycle.open(false);
    lifecycle.open(true);
    await ready();
    pending.resolve(stale);
    await Promise.resolve();
    m.redraw.sync();

    expect(subjects().value).toBe('');
    expect(dsl().value).toBe('');
  });

  it('waits for the latest parse before searching what was typed', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    const pending = deferred<Awaited<ReturnType<typeof client.parseKnowledgeQuery>>>();
    const real = client.parseKnowledgeQuery.bind(client);
    const interpretation = await real({ text: 'about: fusion need: definition' });
    mount({ client });
    await ready();
    vi.spyOn(client, 'parseKnowledgeQuery').mockImplementationOnce(() => pending.promise);

    type(dsl(), 'about: fusion need: definition');
    submitSearch();
    expect(execute).not.toHaveBeenCalled();
    pending.resolve(interpretation);

    await vi.waitFor(() => expect(execute).toHaveBeenCalled());
    expect(execute.mock.calls[0]?.[0]?.draft.about).toEqual(['fusion']);
    expect(execute.mock.calls[0]?.[0]?.draft.needs).toEqual(['definition']);
  });
});

describe('KnowledgeSearchDialog DSL scopes (task 0206)', () => {
  it('applies a DSL root selector to the visual scope control', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();

    type(dsl(), 'about: retrieval scope: root:mock-knowledge-root-handbook');

    await vi.waitFor(() => expect(scopeSelect().value).toBe('enrolledRoots'));
    const checked = [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-roots input')].map(
      (input) => input.checked,
    );
    expect(checked).toEqual([true, false]);

    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalled());
    expect(execute.mock.calls[0]?.[0]?.scope).toMatchObject({
      kind: 'enrolledRoots',
      enrolledRootIds: ['mock-knowledge-root-handbook'],
    });
  });

  it('applies a DSL whole-library selector and keeps it in the query', async () => {
    mount();
    await ready();

    type(dsl(), 'about: retrieval scope: library');
    await vi.waitFor(() => expect(subjects().value).toBe('retrieval'));
    expect(scopeSelect().value).toBe('entireLibrary');

    type(subjects(), 'retrieval\nfusion');

    await vi.waitFor(() => expect(dsl().value).toContain('scope: library'));
  });

  it('refuses to search an indexed root the host never reported', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();

    type(dsl(), 'about: retrieval scope: root:not-an-indexed-root');

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-scope-issues')?.textContent).toContain(
        'root:not-an-indexed-root is not an authorized indexed root here.',
      ),
    );
    submitSearch();
    expect(execute).not.toHaveBeenCalled();
  });

  it('refuses to search a scope selector the composer cannot apply', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();

    type(dsl(), 'about: retrieval scope: workspace:other-workspace');

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-scope-issues')?.textContent).toContain(
        'workspace:other-workspace cannot be applied from this dialog',
      ),
    );
    submitSearch();
    expect(execute).not.toHaveBeenCalled();
  });

  it('keeps an unusable scope selector visible instead of dropping it', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();
    type(dsl(), 'about: retrieval scope: workspace:other-workspace');
    await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-scope-issues')).not.toBeNull());

    type(subjects(), 'retrieval\nfusion');

    await vi.waitFor(() => expect(dsl().value).toContain('scope: workspace:other-workspace'));
    submitSearch();
    expect(execute).not.toHaveBeenCalled();
  });

  it('re-enables searching once the unusable selector is removed', async () => {
    const client = new MockFileManagerClient();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });
    await ready();
    type(dsl(), 'about: retrieval scope: workspace:other-workspace');
    await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-scope-issues')).not.toBeNull());

    type(dsl(), 'about: retrieval');

    await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-scope-issues')).toBeNull());
    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledOnce());
    expect(root.querySelector('.fm-knowledge-scope-issues')).toBeNull();
  });
});

describe('KnowledgeSearchDialog open lifecycle (task 0206)', () => {
  it('focuses the subject field when a closed dialog is opened', async () => {
    const lifecycle = mountLifecycle();
    expect(document.activeElement).not.toBe(root.querySelector('#fm-knowledge-subjects'));

    lifecycle.open(true);
    await ready();

    expect(document.activeElement).toBe(subjects());
  });

  it('puts the caret after a prefilled subject rather than selecting nothing', async () => {
    const lifecycle = mountLifecycle({ initialSubject: 'retrieval' });

    lifecycle.open(true);
    await ready();

    expect(document.activeElement).toBe(subjects());
    expect(subjects().selectionStart).toBe('retrieval'.length);
  });

  it('focuses the subject field again on every reopen', async () => {
    const lifecycle = mountLifecycle();
    lifecycle.open(true);
    await ready();
    dsl().focus();
    expect(document.activeElement).toBe(dsl());

    lifecycle.open(false);
    lifecycle.open(true);
    await ready();

    expect(document.activeElement).toBe(subjects());
  });

  it('closes on Escape from the subject field', async () => {
    const onClose = vi.fn();
    mount({ onClose });
    await ready();

    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(onClose).toHaveBeenCalledOnce();
  });

  it('closes only search settings on Escape from the query-language field', async () => {
    const onClose = vi.fn();
    mount({ onClose });
    await ready();
    root.querySelector<HTMLButtonElement>('button[aria-label="Search settings"]')?.click();
    m.redraw.sync();

    dsl().dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-settings-modal.active')).toBeNull();
    expect(root.querySelector('.fm-knowledge-search')).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('never lets a composer key reach the rest of the application', async () => {
    mount();
    await ready();
    const seen: string[] = [];
    const listener = (event: Event): void => {
      seen.push((event as KeyboardEvent).key);
    };
    document.addEventListener('keydown', listener);

    dsl().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }));
    document.removeEventListener('keydown', listener);

    expect(seen).toEqual([]);
  });
});

describe('KnowledgeSearchDialog retrieval trace (task 0206)', () => {
  async function searchWithTrace(client?: MockFileManagerClient): Promise<void> {
    mount({ client: client ?? new MockFileManagerClient(), initialSubject: 'retrieval' });
    await ready();
    const trace = root.querySelector<HTMLInputElement>('.fm-knowledge-trace input');
    trace?.click();
    m.redraw.sync();
    await search();
  }

  it('renders the bounded queries, candidate counts and rank constant', async () => {
    await searchWithTrace();

    const trace = root.querySelector('.fm-knowledge-trace-body');
    expect(trace?.textContent).toContain('Retrieval trace');
    expect(trace?.textContent).toContain('rank constant 60');
    expect(trace?.textContent).toContain('full-text candidate(s)');
    expect(trace?.querySelector('code')?.textContent).toBe('retrieval');
    expect(trace?.textContent).toContain('bounded query(ies)');
  });

  it('reports the fused row count alongside the traced queries', async () => {
    await searchWithTrace();

    const rows = root.querySelectorAll('.fm-knowledge-result').length;
    expect(root.querySelector('.fm-knowledge-trace-body')?.textContent).toContain(
      `${rows} fused row(s)`,
    );
  });

  it('does not render a trace that was never requested', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(root.querySelector('.fm-knowledge-trace-body')).toBeNull();
  });

  it('says so when a host returns no trace for a traced search', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => ({
      ...(await original(request, signal)),
      trace: null,
    }));

    await searchWithTrace(client);

    expect(root.querySelector('.fm-knowledge-trace-body')?.textContent).toContain(
      'This host returned no retrieval trace',
    );
  });
});

describe('knowledgeProvenanceLabel spreadsheet ranges (task 0206)', () => {
  it('renders production exact and span wrappers as clickable source positions', () => {
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'exact',
          value: { kind: 'pdfBlock', page_number: 167, block_index: 3 },
        }),
      ),
    ).toBe('Page 167');
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'span',
          value: {
            first: { kind: 'pdfBlock', page_number: 167, block_index: 3 },
            last: { kind: 'pdfBlock', page_number: 168, block_index: 1 },
          },
        }),
      ),
    ).toBe('Page 167 – Page 168');
  });

  it('labels reflowable EPUB evidence by chapter and source lines', () => {
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'epubText',
          spine_index: 2,
          start_line: 7,
          end_line: 11,
        }),
      ),
    ).toBe('Chapter 3, lines 7–11');
  });

  it('labels a sheet cell range', () => {
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'spreadsheetRange',
          sheet: 'Q3 Revenue',
          start_row: 2,
          start_column: 1,
          end_row: 7,
          end_column: 3,
        }),
      ),
    ).toBe('Sheet Q3 Revenue, cells B3:D8');
  });

  it('labels a single cell without repeating it', () => {
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'spreadsheetRange',
          sheet: 'Sheet1',
          start_row: 0,
          start_column: 0,
          end_row: 0,
          end_column: 0,
        }),
      ),
    ).toBe('Sheet Sheet1, cells A1');
  });

  it('labels a CSV range that has no sheet name', () => {
    expect(
      knowledgeProvenanceLabel(
        JSON.stringify({
          kind: 'spreadsheetRange',
          sheet: '',
          start_row: 0,
          start_column: 26,
          end_row: 4,
          end_column: 27,
        }),
      ),
    ).toBe('Cells AA1:AB5');
  });

  it('renders nothing rather than an invented range for a malformed provenance', () => {
    expect(
      knowledgeProvenanceLabel(JSON.stringify({ kind: 'spreadsheetRange', sheet: 'Sheet1' })),
    ).toBe('');
  });

  it('keeps structural provenance on the evidence opened from its document', async () => {
    const client = new MockFileManagerClient();
    const onOpenSource = vi.fn();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const [first] = result.evidence;
      if (first === undefined) return result;
      return {
        ...result,
        evidence: [
          {
            ...first,
            unavailable: false,
            provenance: JSON.stringify({
              kind: 'spreadsheetRange',
              sheet: 'Budget',
              start_row: 0,
              start_column: 0,
              end_row: 3,
              end_column: 2,
            }),
          },
        ],
      };
    });
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await ready();

    await search();
    root.querySelector<HTMLButtonElement>('.fm-knowledge-source-link:not(:disabled)')?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({
        provenance: expect.stringContaining('"sheet":"Budget"'),
      }),
    );
  });
});

describe('KnowledgeSearchDialog capability independence (task 0206)', () => {
  it('never renders an answer panel before a search has succeeded (task 0207)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: true,
      semantic: true,
      answerGeneration: true,
    });
    mount({ client, initialSubject: 'retrieval' });
    await vi.waitFor(() => expect(root.textContent).toContain('What do you need?'));

    expect(root.textContent).not.toContain('Generate answer');
    expect(root.querySelector('.fm-knowledge-answer')).toBeNull();
    expect(root.querySelector('.fm-rag-answer')).toBeNull();
    expect(root.textContent).not.toContain('Search never generates an answer');
  });

  it('keeps semantic retrieval usable when full text is unavailable', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: false,
      semantic: true,
      answerGeneration: false,
    });
    mount({ client });
    await ready();

    const modes = [...root.querySelectorAll<HTMLButtonElement>('.fm-knowledge-mode')];
    expect(modes.find((mode) => mode.textContent === 'Semantic')?.disabled).toBe(false);
    expect(modes.find((mode) => mode.textContent === 'Full text')?.disabled).toBe(true);
    expect(root.textContent).toContain('Full-text retrieval is unavailable.');
  });

  it('refreshes stale startup capabilities from the completed search', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: false,
      semantic: true,
      answerGeneration: false,
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    expect(root.textContent).toContain('Full-text retrieval is unavailable.');

    await search();

    expect(root.textContent).not.toContain('Full-text retrieval is unavailable.');
  });

  it('stays fully usable for search with no retrieval capability reported at all', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: true,
      semantic: false,
      answerGeneration: false,
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
    expect(root.textContent).not.toContain('Search never generates an answer');
  });
});

/** Saves one generation profile so the mock host can answer (task 0207). */
async function configureProfile(
  client: MockFileManagerClient,
  options: { readonly name?: string; readonly baseUrl?: string } = {},
): Promise<{ readonly id: string; readonly name: string }> {
  const profile = await client.createLlmProfile({
    name: options.name ?? 'Local profile',
    preset: 'openAiCompatible',
    baseUrl: options.baseUrl ?? 'http://localhost:11434',
    deployment: null,
    apiVersion: null,
    model: 'mock-model',
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
  return { id: profile.id, name: profile.name };
}

function answerSection(): HTMLElement | null {
  return root.querySelector<HTMLElement>('.fm-knowledge-answer');
}

function profileSelect(): HTMLSelectElement {
  const element = root.querySelector<HTMLSelectElement>('#fm-knowledge-answer-profile');
  if (element === null) throw new Error('answer profile select not rendered');
  return element;
}

function selectProfile(profileId: string): void {
  const select = profileSelect();
  select.value = profileId;
  select.dispatchEvent(new Event('change', { bubbles: true }));
  m.redraw.sync();
}

function modelKnowledgeCheckbox(): HTMLInputElement {
  const element = root.querySelector<HTMLInputElement>('#fm-knowledge-model-knowledge');
  if (element === null) throw new Error('model knowledge checkbox not rendered');
  return element;
}

/** Waits until an answer section exists after a completed search. */
async function generateAnswer(): Promise<void> {
  button('Generate answer').click();
  m.redraw.sync();
  await vi.waitFor(() =>
    expect(
      root.querySelector('.fm-knowledge-answer-markdown, .fm-knowledge-answer-error'),
    ).not.toBeNull(),
  );
  m.redraw.sync();
}

/** A knowledge answer whose fields the test controls exactly. */
function answerFixture(
  fingerprint: string,
  overrides: Partial<KnowledgeAnswer> = {},
): KnowledgeAnswer {
  return {
    requestId: 'answer-request',
    evidenceFingerprint: fingerprint,
    profileId: 'profile',
    profileName: 'Local profile',
    locality: 'loopback',
    text: 'Grounded answer [E1].',
    citations: [],
    modelKnowledgeAllowed: false,
    insufficient: false,
    withheldUnauthorized: 0,
    staleEvidence: 0,
    unavailableEvidence: 0,
    ...overrides,
  };
}

/** Waits until a dialog that also loads generation profiles is ready. */
async function answersReady(): Promise<void> {
  await vi.waitFor(() => {
    expect(root.textContent).not.toContain('Loading knowledge search…');
    expect(subjects().disabled).toBe(false);
  });
  m.redraw.sync();
}

describe('KnowledgeSearchDialog optional answers (task 0207)', () => {
  it('never offers answer controls when the host reports no answer capability', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(answerSection()).toBeNull();
    expect(root.textContent).not.toContain('Generate answer');
    expect(root.textContent).not.toContain('Search never generates an answer');
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('does not load generation profiles when answering is unavailable', async () => {
    const client = new MockFileManagerClient();
    const profiles = vi.spyOn(client, 'listLlmProfiles');
    mount({ client, initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(profiles).not.toHaveBeenCalled();
  });

  it('stays neutral when the host offers answers but no profile is saved', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: true,
      semantic: false,
      answerGeneration: true,
    });
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      return { ...result, capabilities: { ...result.capabilities, answerGeneration: true } };
    });
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();

    await search();

    expect(answerSection()).not.toBeNull();
    expect(root.querySelector('#fm-knowledge-answer-profile')).toBeNull();
    expect(root.textContent).toContain('No generation profile is configured');
    expect(() => button('Generate answer')).toThrow();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('keeps the answer section out of the composer until a search has succeeded', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();

    expect(answerSection()).toBeNull();
    expect(root.textContent).not.toContain('Generate answer');

    await search();

    expect(answerSection()).not.toBeNull();
    expect(root.textContent).not.toContain('Search never generates an answer');
  });

  it('requires an explicit profile choice before an answer can be generated', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();

    expect(profileSelect().value).toBe('');
    expect(button('Generate answer').disabled).toBe(true);

    selectProfile(profile.id);

    expect(button('Generate answer').disabled).toBe(false);
  });

  it('answers from the displayed evidence set without searching again', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    type(dsl(), 'about: retrieval do: apply to: "write a scheduler" format: steps depth: brief');
    await vi.waitFor(() => expect(subjects().value).toBe('retrieval'));
    let displayedFingerprint = '';
    const executeOriginal = client.executeKnowledgeSearch.bind(client);
    const execute = vi
      .spyOn(client, 'executeKnowledgeSearch')
      .mockImplementation(async (request, signal) => {
        const executed = await executeOriginal(request, signal);
        displayedFingerprint = executed.evidenceFingerprint;
        return executed;
      });
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    await search();
    const searches = execute.mock.calls.length;
    selectProfile(profile.id);

    await generateAnswer();

    expect(execute.mock.calls.length).toBe(searches);
    expect(generate).toHaveBeenCalledOnce();
    const request = generate.mock.calls[0]?.[0];
    expect(request?.evidenceFingerprint).toBe(displayedFingerprint);
    expect(request?.workspaceId).toBe(workspaceId);
    expect(request?.profileId).toBe(profile.id);
    expect(request?.allowModelKnowledge).toBe(false);
    expect(request?.action).toBe('apply');
    expect(request?.context).toBe('write a scheduler');
    expect(request?.output).toBe('steps');
    expect(request?.depth).toBe('brief');
    expect(root.querySelector('.fm-knowledge-answer-markdown')?.textContent).toContain(
      'Answered from',
    );
  });

  it('carries answer-only constraints without adding them to retrieval', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    type(dsl(), 'about: retrieval constraint: "cite every claim"');
    await vi.waitFor(() => expect(subjects().value).toBe('retrieval'));
    let plannedTexts: readonly string[] = [];
    const executeOriginal = client.executeKnowledgeSearch.bind(client);
    const execute = vi
      .spyOn(client, 'executeKnowledgeSearch')
      .mockImplementation(async (request, signal) => {
        const executed = await executeOriginal(request, signal);
        plannedTexts = executed.plan.searches.map((planned) => planned.text);
        return executed;
      });
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    await search();
    selectProfile(profile.id);

    await generateAnswer();

    expect(generate.mock.calls[0]?.[0]?.constraints).toEqual(['cite every claim']);
    expect(execute.mock.calls[0]?.[0]?.draft.constraints).toEqual(['cite every claim']);
    expect(plannedTexts.length).toBeGreaterThan(0);
    for (const planned of plannedTexts) {
      expect(planned).not.toContain('cite every claim');
    }
  });

  it('keeps generation grounded by default and labels an explicit opt-in', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);

    expect(modelKnowledgeCheckbox().checked).toBe(false);
    modelKnowledgeCheckbox().click();
    m.redraw.sync();
    expect(root.textContent).toContain('not supported by the citations');

    await generateAnswer();

    expect(generate.mock.calls[0]?.[0]?.allowModelKnowledge).toBe(true);
    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'permitted to use general model knowledge',
    );
  });

  it('discloses a cloud endpoint before any evidence is sent', async () => {
    const client = new MockFileManagerClient();
    const local = await configureProfile(client, {
      name: 'Local profile',
      baseUrl: 'http://localhost:11434',
    });
    const cloud = await configureProfile(client, {
      name: 'Cloud profile',
      baseUrl: 'https://llm.example.test',
    });
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();

    selectProfile(local.id);
    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'stays on this device',
    );

    selectProfile(cloud.id);
    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'sends the evidence above',
    );
  });

  it('tells the user to search again when the inspected evidence is gone', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    vi.spyOn(client, 'generateKnowledgeAnswer').mockRejectedValue(
      Object.assign(new Error('gone'), { code: 'knowledgeEvidenceRefreshRequired' }),
    );
    selectProfile(profile.id);

    button('Generate answer').click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-answer [role="alert"]')?.textContent).toContain(
        'Run Search again',
      ),
    );
    expect(execute).not.toHaveBeenCalled();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('cancels a running generation and reports the cancellation', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(
      (_request, signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener('abort', () =>
            reject(new DOMException('The operation was aborted.', 'AbortError')),
          );
        }),
    );

    button('Generate answer').click();
    m.redraw.sync();
    expect(root.textContent).toContain('Generating answer…');
    button('Cancel answer').click();

    await vi.waitFor(() => expect(root.textContent).toContain('The answer was cancelled.'));
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('reports a generation failure without losing the results', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockRejectedValue(new Error('boom'));
    selectProfile(profile.id);

    button('Generate answer').click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-answer [role="alert"]')?.textContent).toContain(
        'The answer could not be generated.',
      ),
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('reports a profile-loading failure and keeps search complete', async () => {
    const client = new MockFileManagerClient({
      failures: { listLlmProfiles: new Error('offline') },
    });
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();

    await search();

    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'Generation profiles could not be loaded.',
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
    expect(root.querySelector('#fm-knowledge-answer-profile')).toBeNull();
  });

  it('clears a generated answer as soon as the query is edited', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);
    await generateAnswer();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).not.toBeNull();

    type(subjects(), 'fusion');

    expect(answerSection()).toBeNull();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('clears a generated answer when a new search is started', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);
    await generateAnswer();

    await search();

    await vi.waitFor(() =>
      expect(root.querySelector('#fm-knowledge-answer-profile')).not.toBeNull(),
    );
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('discards an answer that lands after a further edit', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    const pending = deferred<KnowledgeAnswer>();
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(() => pending.promise);

    button('Generate answer').click();
    m.redraw.sync();
    type(subjects(), 'fusion');
    pending.resolve(answerFixture('stale-fingerprint'));
    await Promise.resolve();
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
    expect(root.textContent).not.toContain('Grounded answer');
  });

  it('discards an answer that lands after the dialog was closed and reopened', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    const pending = deferred<KnowledgeAnswer>();
    const lifecycle = mountLifecycle({ client, initialSubject: 'retrieval' });
    lifecycle.open(true);
    await answersReady();
    await search();
    selectProfile(profile.id);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(() => pending.promise);
    button('Generate answer').click();
    m.redraw.sync();

    lifecycle.open(false);
    lifecycle.open(true);
    await answersReady();
    pending.resolve(answerFixture('stale-fingerprint'));
    await Promise.resolve();
    m.redraw.sync();

    expect(root.textContent).not.toContain('Grounded answer');
    expect(answerSection()).toBeNull();
  });

  it('opens the exact displayed source behind a citation', async () => {
    const onOpenSource = vi.fn();
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await answersReady();
    await search();
    selectProfile(profile.id);
    await generateAnswer();

    const citation = root.querySelector<HTMLButtonElement>(
      '.fm-knowledge-citations button:not(:disabled)',
    );
    citation?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.stringContaining('mock-knowledge-source-') }),
    );
  });

  it('opens a citation from the answer text itself', async () => {
    const onOpenSource = vi.fn();
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await answersReady();
    await search();
    selectProfile(profile.id);
    await generateAnswer();

    const link = root.querySelector<HTMLAnchorElement>('.fm-knowledge-answer-markdown a');
    expect(link?.getAttribute('href')).toContain('#fm-knowledge-citation-');
    link?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.stringContaining('mock-knowledge-source-') }),
    );
  });

  it('never links a citation whose identity was not displayed', async () => {
    const onOpenSource = vi.fn();
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await answersReady();
    await search();
    const original = client.generateKnowledgeAnswer.bind(client);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => {
      const generated = await original(request, signal);
      return {
        ...generated,
        text: 'Invented reference [E9].',
        citations: [
          {
            label: 'E9',
            recordId: 'never-displayed',
            sourceId: 'never-displayed-source',
            provenance: '',
            sectionPath: [],
            finalRank: 9,
            generated: false,
            stale: false,
            unavailable: false,
          },
        ],
      };
    });
    selectProfile(profile.id);

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer-markdown a')).toBeNull();
    const citation = root.querySelector<HTMLButtonElement>('.fm-knowledge-citations button');
    expect(citation?.disabled).toBe(true);
    citation?.click();
    expect(onOpenSource).not.toHaveBeenCalled();
  });

  it('renders answer markdown safely', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    const original = client.generateKnowledgeAnswer.bind(client);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => ({
      ...(await original(request, signal)),
      text: '# Heading\n\n<script>window.pwned = true;</script>\n\n**bold**',
    }));
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    selectProfile(profile.id);

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer-markdown h1')?.textContent).toBe('Heading');
    expect(root.querySelector('.fm-knowledge-answer-markdown strong')?.textContent).toBe('bold');
    expect(root.querySelector('.fm-knowledge-answer-markdown script')).toBeNull();
  });

  it('reports stale, unavailable, generated and withheld provenance honestly', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    const original = client.generateKnowledgeAnswer.bind(client);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => {
      const generated = await original(request, signal);
      const [first, ...rest] = generated.citations;
      if (first === undefined) return generated;
      return {
        ...generated,
        citations: [{ ...first, stale: true, generated: true }, ...rest],
        staleEvidence: 2,
        unavailableEvidence: 1,
        withheldUnauthorized: 3,
      };
    });
    selectProfile(profile.id);

    await generateAnswer();

    const section = root.querySelector('.fm-knowledge-answer');
    expect(section?.textContent).toContain('2 cited source(s) changed since indexing');
    expect(section?.textContent).toContain('1 cited source(s) are currently unavailable');
    expect(section?.textContent).toContain('3 evidence row(s) were withheld');
    expect(root.querySelector('.fm-knowledge-citations')?.textContent).toContain(
      'Source changed since indexing',
    );
    expect(root.querySelector('.fm-knowledge-citations')?.textContent).toContain(
      'Generated summary',
    );
  });

  it('says so when the retained evidence cannot support an answer', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    const original = client.generateKnowledgeAnswer.bind(client);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => ({
      ...(await original(request, signal)),
      insufficient: true,
      citations: [],
      text: 'The inspected evidence is insufficient to answer this request.',
    }));
    selectProfile(profile.id);

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'could not support a grounded answer',
    );
  });

  it('labels every answer control and announces generation politely', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();

    expect(answerSection()?.getAttribute('aria-label')).toBe('Optional knowledge answer');
    expect(root.querySelector('label[for="fm-knowledge-answer-profile"]')?.textContent).toContain(
      'Generation profile',
    );
    expect(root.querySelector('label[for="fm-knowledge-model-knowledge"]')?.textContent).toContain(
      'general model knowledge',
    );
    expect(root.querySelector('.fm-knowledge-answer-body')?.getAttribute('aria-live')).toBe(
      'polite',
    );

    selectProfile(profile.id);
    await generateAnswer();

    expect(document.activeElement).toBe(root.querySelector('.fm-knowledge-answer-body'));
  });

  it('cancels a running generation when the dialog is closed', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureProfile(client);
    const onClose = vi.fn();
    let aborted = false;
    mount({ client, initialSubject: 'retrieval', onClose });
    await answersReady();
    await search();
    selectProfile(profile.id);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(
      (_request, signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener('abort', () => {
            aborted = true;
            reject(new DOMException('The operation was aborted.', 'AbortError'));
          });
        }),
    );

    button('Generate answer').click();
    m.redraw.sync();
    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(onClose).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(aborted).toBe(true));
  });
});
