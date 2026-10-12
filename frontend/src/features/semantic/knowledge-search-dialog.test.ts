import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { KnowledgeAnswer, KnowledgeEvidence } from '../../models';
import {
  groupEvidenceByDocument,
  KnowledgeSearchDialog,
  knowledgeProvenanceLabel,
  numberKnowledgeAnswer,
} from './knowledge-search-dialog';

let root: HTMLElement;

const workspaceId = '22222222-2222-4222-8222-222222222222';
const preferencesStorageKey = 'procyon.knowledgeSearch.preferences.v1';

interface MountOptions {
  readonly client?: MockFileManagerClient;
  readonly currentFolder?:
    | { providerId: string; uri: string }
    | (() => { providerId: string; uri: string });
  readonly semanticSourceIds?: readonly string[];
  readonly initialSubject?: string;
  readonly onClose?: () => void;
  readonly onOpenSource?: (evidence: KnowledgeEvidence) => void | Promise<void>;
  readonly isSourceOpen?: (evidence: KnowledgeEvidence) => boolean;
  readonly profileRevision?: () => number;
  readonly onConfigureAnswers?: () => void;
}

function mount(options: MountOptions = {}): MockFileManagerClient {
  const client = options.client ?? new MockFileManagerClient();
  m.mount(root, {
    view: () =>
      m(KnowledgeSearchDialog, {
        open: true,
        client,
        workspaceId,
        currentFolder:
          typeof options.currentFolder === 'function'
            ? options.currentFolder()
            : options.currentFolder,
        semanticSourceIds: options.semanticSourceIds ?? [],
        initialSubject: options.initialSubject,
        profileRevision: options.profileRevision?.() ?? 0,
        onClose: options.onClose ?? vi.fn(),
        ...(options.onOpenSource === undefined ? {} : { onOpenSource: options.onOpenSource }),
        ...(options.isSourceOpen === undefined ? {} : { isSourceOpen: options.isSourceOpen }),
        ...(options.onConfigureAnswers === undefined
          ? {}
          : { onConfigureAnswers: options.onConfigureAnswers }),
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

function searchButton(): HTMLButtonElement {
  const found = root.querySelector<HTMLButtonElement>('.fm-knowledge-search-submit');
  if (found === null) throw new Error('search button not rendered');
  return found;
}

/** Waits until the dialog finished loading its reported capabilities and roots. */
async function ready(): Promise<void> {
  await vi.waitFor(() => {
    expect(root.textContent).not.toContain('Loading semantic search…');
    expect(root.querySelector('.fm-knowledge-advanced')).not.toBeNull();
  });
  m.redraw.sync();
}

async function search(): Promise<void> {
  submitSearch();
  await vi.waitFor(() => expect(root.querySelector('.fm-knowledge-search-spinner')).toBeNull());
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
  vi.unstubAllGlobals();
});

describe('numberKnowledgeAnswer', () => {
  const evidence = (recordId: string, documentId: string, title: string): KnowledgeEvidence =>
    ({
      recordId,
      documentId,
      sourceId: `source-${recordId}`,
      title,
      sectionPath: [],
    }) as unknown as KnowledgeEvidence;
  const citation = (label: string, row: KnowledgeEvidence) => ({
    label,
    recordId: row.recordId,
    sourceId: row.sourceId,
    sectionPath: [],
    provenance: '',
    finalRank: 1,
    generated: false,
    unavailable: false,
  });
  const a1 = evidence('r1', 'doc-a', 'a.md');
  const b = evidence('r2', 'doc-b', 'b.md');
  const a2 = evidence('r3', 'doc-a', 'a.md');
  const displayed = new Map([a1, b, a2].map((row) => [row.recordId, row]));
  const citations = [citation('E1', a1), citation('E2', b), citation('E3', a2)];

  it('drops a trailing sources line and merges adjacent bracketed citations', () => {
    const numbered = numberKnowledgeAnswer(
      'Sorteer op getal [E2], [E1].\n\nBronnen: [E1], [E2], [E3]',
      citations,
      displayed,
    );
    expect(numbered.text).toBe('Sorteer op getal [1, 2].');
    expect(numbered.references.map((reference) => reference.title)).toEqual(['b.md', 'a.md']);
  });

  it('numbers files in order of first mention and merges sections of one file', () => {
    const numbered = numberKnowledgeAnswer(
      'First E2, E3. Then [E1] and (E2, E1).',
      citations,
      displayed,
    );
    expect(numbered.text).toBe('First [1, 2]. Then [2] and [1, 2].');
    expect(numbered.references.map((reference) => [reference.number, reference.title])).toEqual([
      [1, 'b.md'],
      [2, 'a.md'],
    ]);
    expect(numbered.references[1]?.citations).toHaveLength(2);
  });

  it('leaves unknown labels untouched', () => {
    expect(numberKnowledgeAnswer('See E9.', citations, displayed).text).toBe('See E9.');
  });
});

describe('KnowledgeSearchDialog (task 0206)', () => {
  it('docks Ask below the results when an LLM profile is available', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await askBarReady();

    expect(root.querySelector('.fm-knowledge-surface-modes')).toBeNull();
    const workspace = root.querySelector('.fm-knowledge-workspace');
    expect(workspace?.lastElementChild?.classList.contains('fm-knowledge-answer')).toBe(true);
    expect(root.querySelector('.fm-knowledge-ask-bar #fm-knowledge-question')).not.toBeNull();
    expect(button('Ask').disabled).toBe(true);
    await search();
    expect(root.querySelector('.fm-knowledge-evidence-panel')).not.toBeNull();
    openAsk();
    expect(button('Ask').disabled).toBe(false);
  });

  it('asks with Enter and keeps Shift+Enter for a new line', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();

    const field = question();
    field.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true }),
    );
    m.redraw.sync();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();

    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-answer-markdown')).not.toBeNull(),
    );
  });

  it('keeps prompt focus beside the query and moves retrieval refinements into Advanced', async () => {
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
    expect(
      toolbar?.querySelector<HTMLSelectElement>('select[name="knowledge-intent"]')?.value,
    ).toBe('search');
    expect(root.querySelector('.fm-knowledge-search-options')).toBeNull();
    expect(root.querySelector('.fm-knowledge-composer > .fm-knowledge-advanced')).toBeNull();

    settings?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-settings-modal')).not.toBeNull();
    expect(
      root.querySelector('.fm-knowledge-settings-modal .fm-knowledge-advanced'),
    ).not.toBeNull();
    expect(
      root.querySelector('.fm-knowledge-settings-modal .fm-knowledge-needs input'),
    ).not.toBeNull();
  });

  it('routes selected question and code prompts without rewriting the advanced query', async () => {
    const client = mount({ initialSubject: 'retrieval' });
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    await ready();
    const focus = root.querySelector<HTMLSelectElement>('select[name="knowledge-intent"]');
    if (focus === null) throw new Error('search focus selector not rendered');
    focus.value = 'questionAnswering';
    focus.dispatchEvent(new Event('change', { bubbles: true }));
    await search();
    expect(execute.mock.calls[0]?.[0]?.intent).toBe('questionAnswering');

    focus.value = 'codeRetrieval';
    focus.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-knowledge-results-heading[role="status"]')).toBeNull();
    type(dsl(), 'about: "index implementation"');
    await vi.waitFor(() => expect(subjects().value).toContain('index implementation'));
    expect(root.querySelector<HTMLSelectElement>('select[name="knowledge-intent"]')?.value).toBe(
      'codeRetrieval',
    );
    await search();
    expect(execute.mock.calls[1]?.[0]?.intent).toBe('codeRetrieval');
  });

  it('prefills Ask from a single question without generating an answer on search', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'Why do turbines spin?' });
    await answersReady();
    const focus = root.querySelector<HTMLSelectElement>('select[name="knowledge-intent"]');
    if (focus === null) throw new Error('search focus selector not rendered');
    focus.value = 'questionAnswering';
    focus.dispatchEvent(new Event('change', { bubbles: true }));
    await search();

    expect(question().value).toBe('Why do turbines spin?');
    expect(generate).not.toHaveBeenCalled();
  });

  it('offers one-way inclusion for an unindexed current folder', async () => {
    const currentFolder = { providerId: 'local', uri: 'file:///documents' };
    let displayedFolder = currentFolder;
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'notIncluded',
      rootId: null,
      exclusionId: null,
      workspaceReferenced: false,
      sourceAvailable: true,
      unavailableReason: null,
    });
    mount({ client, currentFolder: () => displayedFolder });

    await ready();

    const include = root.querySelector<HTMLButtonElement>('.fm-knowledge-include-folder-action');
    expect(include?.textContent).toBe('Include folder for semantic search');
    const confirm = vi.spyOn(client, 'confirmSemanticEnrolment');
    const preview = vi.spyOn(client, 'previewSemanticEnrolment');
    include?.focus();
    expect(document.activeElement).toBe(include);
    expect(confirm).not.toHaveBeenCalled();
    displayedFolder = { providerId: 'local', uri: 'file:///other' };
    m.redraw.sync();
    include?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-folder-prompt')).not.toBeNull());
    await vi.waitFor(() =>
      expect(preview).toHaveBeenCalledWith(expect.objectContaining({ location: currentFolder })),
    );
    expect(confirm).not.toHaveBeenCalled();
  });

  it('does not offer folder removal when the current folder is already included', async () => {
    const currentFolder = { providerId: 'local', uri: 'file:///documents' };
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'includedHere',
      rootId: 'root-1',
      exclusionId: null,
      workspaceReferenced: true,
      sourceAvailable: true,
      unavailableReason: null,
    });
    mount({ client, currentFolder });

    expect(root.querySelector('.fm-knowledge-include-folder-action')).toBeNull();

    expect(root.querySelector('.fm-knowledge-search-options')).toBeNull();
  });

  it('closes the transient search pane from the toolbar close button', async () => {
    const onClose = vi.fn();
    mount({ onClose });

    await ready();
    button('Close Semantic Search').click();

    expect(onClose).toHaveBeenCalledOnce();
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

  it('accepts a query while loading and runs a queued Enter once ready', async () => {
    const client = new MockFileManagerClient();
    const original = client.listKnowledgeRoots.bind(client);
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.spyOn(client, 'listKnowledgeRoots').mockImplementation(async (request) => {
      await gate;
      return original(request);
    });
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client });

    expect(subjects().disabled).toBe(false);
    type(subjects(), 'retrieval');
    submitSearch();
    expect(execute).not.toHaveBeenCalled();

    release();
    await ready();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(1));
    expect(subjects().value).toBe('retrieval');
  });

  it('shows and enforces the empty-index state before a query is submitted', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([]);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    mount({ client, initialSubject: 'retrieval' });

    await ready();

    expect(root.textContent).toContain(
      'No indexed documents are available in this scope. Choose another scope or index a folder, then try again.',
    );
    expect(searchButton().disabled).toBe(true);
    submitSearch();
    expect(execute).not.toHaveBeenCalled();
  });

  it('distinguishes an enrolled root whose initial indexing has not completed', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([
      {
        rootId: 'pending-root',
        label: '/Users/me/OneDrive/Basisschool',
        location: { providerId: 'local', uri: 'file:///Users/me/OneDrive/Basisschool' },
        recursive: true,
        indexedGeneration: 0,
        indexedSources: 2,
        confirmedCompletedSources: 0,
        unverifiedLegacySources: 0,
        available: true,
      },
    ]);
    mount({ client, initialSubject: 'retrieval' });

    await ready();

    expect(root.textContent).toContain(
      'The folder “Basisschool” is included and still being indexed. Search becomes available after the first document is indexed.',
    );
    expect(root.textContent).not.toContain('/Users/me/OneDrive/Basisschool is included');
    expect(root.textContent).not.toContain('No indexed documents are available in this scope.');
    expect(searchButton().disabled).toBe(true);
  });

  it('enables search when the first indexed source arrives without reopening the pane', async () => {
    const client = new MockFileManagerClient();
    const indexedSources = 1;
    let confirmedCompletedSources = 0;
    const roots = vi.spyOn(client, 'listKnowledgeRoots').mockImplementation(async () => [
      {
        rootId: 'new-root',
        label: 'New folder',
        location: { providerId: 'local', uri: 'file:///new-folder' },
        recursive: true,
        indexedGeneration: 0,
        indexedSources,
        confirmedCompletedSources,
        unverifiedLegacySources: 0,
        available: true,
      },
    ]);
    await client.subscribe(() => undefined);
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    expect(searchButton().disabled).toBe(true);
    confirmedCompletedSources = 1;
    client.emit({
      eventId: 1,
      timestamp: new Date().toISOString(),
      payload: {
        type: 'semantic.ingestionProgress',
        jobId: 'root:new-root',
        stage: 'reconciling',
        completed: 4,
        total: 0,
        errors: 0,
      },
    });
    await vi.waitFor(() => expect(roots).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(searchButton().disabled).toBe(false));
  });

  it('does not call a root pending once it already has searchable documents', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([
      {
        rootId: 'partial-root',
        label: '/Users/me/OneDrive/Basisschool',
        location: { providerId: 'local', uri: 'file:///Users/me/OneDrive/Basisschool' },
        recursive: true,
        indexedGeneration: 0,
        indexedSources: 42,
        confirmedCompletedSources: 42,
        unverifiedLegacySources: 0,
        available: true,
      },
    ]);
    mount({ client, initialSubject: 'retrieval' });

    await ready();

    expect(root.textContent).not.toContain('still being indexed');
    expect(searchButton().disabled).toBe(false);
  });

  it('keeps search available for a pre-upgrade index without inventing completion receipts', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([
      {
        rootId: 'legacy-root',
        label: 'Previously indexed',
        location: { providerId: 'local', uri: 'file:///previously-indexed' },
        recursive: true,
        indexedGeneration: 1,
        indexedSources: 3,
        confirmedCompletedSources: 0,
        unverifiedLegacySources: 3,
        available: true,
      },
    ]);
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    expect(searchButton().disabled).toBe(false);
  });

  it('names every enrolled root that is still being indexed', async () => {
    const client = new MockFileManagerClient();
    const pending = (name: string) => ({
      rootId: `pending-${name}`,
      label: name,
      location: { providerId: 'local', uri: `file:///Users/me/${encodeURIComponent(name)}` },
      recursive: true,
      indexedGeneration: 0,
      indexedSources: 0,
      confirmedCompletedSources: 0,
      unverifiedLegacySources: 0,
      available: true,
    });
    vi.spyOn(client, 'listKnowledgeRoots').mockResolvedValue([
      pending('semantic'),
      pending('Research notes'),
    ]);
    mount({ client, initialSubject: 'retrieval' });

    await ready();

    expect(root.textContent).toContain(
      'The folders “semantic”, “Research notes” are included and still being indexed. Search becomes available after the first document is indexed.',
    );
  });

  it('keeps answer-generation copy out of the common flow when it is unavailable', async () => {
    mount({ initialSubject: 'retrieval' });

    await ready();
    expect(root.textContent).not.toContain('Search never generates an answer');
    expect(root.querySelector('.fm-knowledge-generate')).toBeNull();
    expect(root.textContent).not.toContain('Your generated answer will appear here.');
    expect(root.querySelector('.fm-rag-answer')).toBeNull();
  });

  it('offers every knowledge need and keeps current folder as a separate filter', async () => {
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
      'All indexed folders in this workspace',
      'Selected indexed roots',
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
    expect(root.textContent).toContain('Scope: 2 indexed root(s)');
  });

  it('runs a complete search with no language model as ranked documents', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();
    root.querySelectorAll<HTMLInputElement>('.fm-knowledge-needs input')[1]?.click();
    m.redraw.sync();

    await search();

    expect(root.querySelector('.fm-knowledge-results-section')?.getAttribute('aria-label')).toBe(
      'Semantic search results',
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

  it('renders decoded document-ranked PDF excerpts without interpreting their text as Markdown', async () => {
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
            content: '## Su-Field model\n\nA **substance-field**\nsection.',
            mediaType: 'application/pdf',
            fusedScore: 0.023456,
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
            mediaType: 'application/pdf',
            fusedScore: 0.012345,
            finalRank: 8,
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
    const relevance = [...root.querySelectorAll<HTMLElement>('.fm-knowledge-relevance-indicator')];
    expect(relevance.map((indicator) => indicator.getAttribute('aria-label'))).toEqual([
      'High relevance',
      'Medium relevance',
    ]);
    expect(relevance.map((indicator) => indicator.tabIndex)).toEqual([0, 0]);
    expect(relevance.map((indicator) => indicator.parentElement?.dataset.tooltip)).toEqual([
      'High relevance',
      'Medium relevance',
    ]);
    expect(relevance.map((indicator) => indicator.dataset.strength)).toEqual(['high', 'medium']);
    expect(root.textContent).not.toContain('0.023');
    expect(root.textContent).not.toContain('0.012');
    expect(root.textContent).not.toContain('Matching section');
    expect(root.querySelector('.fm-knowledge-result-markdown p')?.textContent).toBe(
      '## Su-Field model\n\nA **substance-field**\nsection.',
    );
    expect(root.querySelector('.fm-knowledge-result-markdown h2')).toBeNull();
    expect(root.querySelector('.fm-knowledge-result-markdown strong')).toBeNull();
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

  it.each([
    [
      'embedded image',
      '{"kind":"exact","value":{"kind":"pdfImage","page_number":3,"image_index":1}}',
      'Embedded PDF image',
      'Page 3 · Image 1',
      'Ingesloten PDF-afbeelding',
      'Pagina 3 · Afbeelding 1',
    ],
    [
      'rendered page',
      '{"kind":"exact","value":{"kind":"pdfPageRender","page_number":3}}',
      'Rendered PDF page (not an embedded image)',
      'Page 3',
      'Gerenderde PDF-pagina (geen ingesloten afbeelding)',
      'Pagina 3',
    ],
  ])(
    'shows page-specific PDF %s evidence without pretending the whole PDF is an image',
    async (_, provenance, badge, citation, dutchBadge, dutchCitation) => {
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
              title: 'scanned.pdf',
              mediaType: 'application/pdf',
              chunkKind: 'pdf_image',
              excerpt: 'PDF page 3, image 1',
              content: '',
              sectionPath: [],
              provenance,
              unavailable: false,
            },
          ],
        };
      });
      const onOpenSource = vi.fn();
      mount({ client, initialSubject: 'retrieval', onOpenSource });
      await ready();
      await search();

      expect(root.querySelector('.fm-knowledge-pdf-image-evidence')?.textContent).toBe(badge);
      const page = root.querySelector<HTMLButtonElement>('.fm-knowledge-page-link');
      expect(page?.textContent).toBe(citation);
      expect(root.querySelector('.fm-knowledge-result-markdown')).toBeNull();
      expect(root.querySelector('.fm-knowledge-image-preview')).toBeNull();
      expect(root.querySelector('.fm-knowledge-media-copy')).toBeNull();
      page?.click();
      expect(onOpenSource).toHaveBeenCalledWith(
        expect.objectContaining({ chunkKind: 'pdf_image', mediaType: 'application/pdf' }),
      );
      setLocale('nl');
      m.redraw.sync();
      expect(root.querySelector('.fm-knowledge-pdf-image-evidence')?.textContent).toBe(dutchBadge);
      expect(root.querySelector('.fm-knowledge-page-link')?.textContent).toBe(dutchCitation);
    },
  );

  it('decodes PDF entities as selectable text and opens only valid web links externally', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    const openExternal = vi.spyOn(client, 'openExternalUrl').mockResolvedValue();
    const openSource = vi.fn();
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'content.pdf',
            mediaType: 'application/pdf',
            unavailable: false,
            sectionPath: [],
            provenance:
              '{"kind":"exact","value":{"kind":"pdfBlock","page_number":1,"block_index":0}}',
            content:
              'Child Abuse &amp; Neglect &le; 3 &#x2264; 4 &lt; 5 <img src=x> https://doi.org/10.1234/example?q=1&amp;page=2. javascript:alert(1)',
          },
        ],
      };
    });
    mount({ client, initialSubject: 'retrieval', onOpenSource: openSource });
    await ready();
    await search();

    const excerpt = root.querySelector<HTMLElement>('.fm-knowledge-result-markdown');
    expect(excerpt?.textContent).toContain('Child Abuse & Neglect ≤ 3 ≤ 4 < 5 <img src=x>');
    expect(excerpt?.querySelector('img')).toBeNull();
    const page = root.querySelector<HTMLButtonElement>(
      '.fm-knowledge-document-heading .fm-knowledge-page-link',
    );
    expect(page?.textContent).toBe('Page 1');
    expect(page?.closest('.fm-knowledge-document-heading')?.textContent).toContain('content.pdf');
    expect(root.querySelector('.fm-knowledge-section-heading')).toBeNull();
    const link = excerpt?.querySelector<HTMLAnchorElement>('a');
    expect(link?.textContent).toBe('https://doi.org/10.1234/example?q=1&page=2');
    expect(link?.href).toBe('https://doi.org/10.1234/example?q=1&page=2');
    expect(excerpt?.querySelectorAll('a')).toHaveLength(1);
    expect(excerpt?.textContent).toContain('page=2. javascript:alert(1)');

    const selection = vi.spyOn(document, 'getSelection').mockReturnValue({
      toString: () => 'Child Abuse',
    } as Selection);
    link?.click();
    expect(openExternal).not.toHaveBeenCalled();
    selection.mockReturnValue(null);
    link?.click();
    expect(openExternal).toHaveBeenCalledOnce();
    expect(openExternal).toHaveBeenCalledWith('https://doi.org/10.1234/example?q=1&page=2');
    expect(openSource).not.toHaveBeenCalled();
    page?.click();
    expect(openSource).toHaveBeenCalledWith(expect.objectContaining({ title: 'content.pdf' }));

    openExternal.mockRejectedValueOnce(new Error('No browser available'));
    link?.click();
    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toContain(
        'The link could not be opened in your default browser.',
      ),
    );
  });

  it('copies a displayed PDF chunk as decoded text with a source reference, never markup', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'content.pdf',
            mediaType: 'application/pdf',
            provenance:
              '{"kind":"exact","value":{"kind":"pdfBlock","page_number":1,"block_index":0}}',
            sectionPath: [],
            content: 'Child Abuse &amp; Neglect &le; 3 https://doi.org/10.1234/example',
          },
        ],
      };
    });
    const write = vi.fn().mockResolvedValue(undefined);
    const oldClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: write },
    });
    try {
      mount({ client, initialSubject: 'retrieval' });
      await ready();
      await search();
      const copy = button('Copy text chunk');
      copy.click();
      await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
      expect(write).toHaveBeenCalledWith(
        'Child Abuse & Neglect ≤ 3 https://doi.org/10.1234/example\n\nSource: content.pdf, Page 1',
      );
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-knowledge-chunk-copy [role="status"]')?.textContent).toBe(
          'Text chunk copied',
        ),
      );
      setLocale('nl');
      m.redraw.sync();
      button('Tekstfragment kopiëren').click();
      await vi.waitFor(() =>
        expect(write).toHaveBeenLastCalledWith(
          'Child Abuse & Neglect ≤ 3 https://doi.org/10.1234/example\n\nBron: content.pdf, Pagina 1',
        ),
      );
    } finally {
      if (oldClipboard === undefined) Reflect.deleteProperty(navigator, 'clipboard');
      else Object.defineProperty(navigator, 'clipboard', oldClipboard);
    }
  });

  it('repairs an already-indexed split Elsevier DOI without joining unrelated prose', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    const openExternal = vi.spyOn(client, 'openExternalUrl').mockResolvedValue();
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (!first) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            mediaType: 'application/pdf',
            content:
              'https://doi.org/10.1016/j. chiabu.2011.01.002. Another sentence: j. chiabu.2011.01.002 is not a DOI. https://doi.org/10.1016/j.chiabu.2018.07.034.',
          },
        ],
      };
    });
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    await search();
    const excerpt = root.querySelector('.fm-knowledge-result-markdown');
    const links = [...(excerpt?.querySelectorAll<HTMLAnchorElement>('a') ?? [])];
    expect(links.map((link) => link.href)).toEqual([
      'https://doi.org/10.1016/j.chiabu.2011.01.002',
      'https://doi.org/10.1016/j.chiabu.2018.07.034',
    ]);
    expect(excerpt?.textContent).toContain('j. chiabu.2011.01.002 is not a DOI');
    links[0]?.click();
    expect(openExternal).toHaveBeenCalledWith('https://doi.org/10.1016/j.chiabu.2011.01.002');
  });

  it.each([
    ['image/png', 'Image', 'Open image'],
    ['audio/mpeg', 'Audio', 'Open audio'],
    ['video/mp4', 'Video', 'Open video'],
  ])(
    'presents %s as media evidence with authorized source navigation',
    async (mediaType, label, action) => {
      const client = new MockFileManagerClient();
      const original = client.executeKnowledgeSearch.bind(client);
      vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
        const result = await original(request, signal);
        const first = result.evidence[0];
        if (first === undefined) throw new Error('missing knowledge fixture');
        return {
          ...result,
          evidence: [
            {
              ...first,
              mediaType,
              modifiedAtMs: Date.parse('2026-04-12T09:30:00.000Z'),
              content: '# <|image|> **not markdown**',
              excerpt: 'A garden path beside a stone wall.',
              unavailable: false,
              provenance: '',
              sectionPath: [],
            },
          ],
        };
      });
      const onOpenSource = vi.fn();
      mount({ client, initialSubject: 'retrieval', onOpenSource });
      await ready();
      await search();

      const media = root.querySelector('.fm-knowledge-media-evidence');
      expect(media?.textContent).toContain(label);
      expect(media?.textContent).toContain('Modified');
      expect(media?.textContent).toContain('A garden path beside a stone wall.');
      expect(media?.textContent).not.toContain('<|image|>');
      expect(root.querySelector('.fm-knowledge-result-markdown')).toBeNull();
      root.querySelector<HTMLButtonElement>('.fm-knowledge-media-open')?.click();
      expect(root.querySelector<HTMLButtonElement>('.fm-knowledge-media-open')?.textContent).toBe(
        action,
      );
      expect(onOpenSource).toHaveBeenCalledWith(expect.objectContaining({ mediaType }));
    },
  );

  it('keeps a file-primary image result compact without hiding its relevance or action', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'page_019.png',
            mediaType: 'image/png',
            modifiedAtMs: Date.parse('2026-05-04T17:14:00.000Z'),
            excerpt: 'image: page_019.png (file-level evidence)',
            sectionPath: [],
            provenance: '',
          },
        ],
      };
    });
    mount({ client, initialSubject: 'retrieval', onOpenSource: vi.fn() });
    await ready();
    await search();

    const heading = root.querySelector('.fm-knowledge-document-heading');
    expect(heading?.textContent).toContain('page_019.png');
    expect(heading?.querySelector('.fm-knowledge-relevance-indicator')).not.toBeNull();
    expect(
      heading?.querySelector('.fm-knowledge-relevance-indicator')?.getAttribute('aria-label'),
    ).toContain('relevance');
    expect(root.querySelector('.fm-knowledge-group-compact-media')).not.toBeNull();
    expect(root.querySelector('.fm-knowledge-section-heading')).toBeNull();
    const summary = root.querySelector('.fm-knowledge-media-summary');
    expect(summary?.textContent).toContain('Modified');
    expect(summary?.querySelector('.fm-knowledge-media-open')?.textContent).toBe('Open image');
    expect(root.querySelector('.fm-knowledge-media-evidence p')).toBeNull();
  });

  it('copies authorized image bytes, not its preview, only after a successful clipboard write', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'garden.png',
            mediaType: 'image/png',
            sectionPath: [],
            provenance: '',
            unavailable: false,
            stale: false,
          },
        ],
      };
    });
    vi.spyOn(client, 'resolveKnowledgeSource').mockResolvedValue({
      available: true,
      entryId: 'garden',
      location: { providerId: 'local', uri: 'file:///garden.png' },
    });
    const data = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    const read = vi.spyOn(client, 'readFileRange').mockResolvedValue({
      data,
      offset: 0,
      length: data.length,
      eof: true,
      probablyBinary: true,
    });
    const write = vi.fn().mockResolvedValue(undefined);
    class ClipboardItemStub {
      constructor(readonly items: Record<string, Blob>) {}
    }
    vi.stubGlobal('ClipboardItem', ClipboardItemStub);
    const oldClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { write } });
    try {
      mount({ client, initialSubject: 'retrieval' });
      await ready();
      await search();
      button('Copy image').click();
      await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
      expect(read).toHaveBeenCalledWith(
        {
          location: { providerId: 'local', uri: 'file:///garden.png' },
          offset: 0,
          length: 1048576,
        },
        expect.any(AbortSignal),
      );
      const item = write.mock.calls[0]?.[0]?.[0] as ClipboardItemStub;
      const blob = item.items['image/png'];
      if (blob === undefined) throw new Error('missing image/png clipboard payload');
      expect([...new Uint8Array(await blob.arrayBuffer())]).toEqual(data);
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-knowledge-media-copy [role="status"]')?.textContent).toBe(
          'Image copied',
        ),
      );
    } finally {
      if (oldClipboard === undefined) Reflect.deleteProperty(navigator, 'clipboard');
      else Object.defineProperty(navigator, 'clipboard', oldClipboard);
    }
  });

  it('copies audio details with a local link, but never treats a remote URI as portable', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'birds.mp3',
            mediaType: 'audio/mpeg',
            excerpt: 'Birds &amp; wind',
            sectionPath: [],
            provenance: '',
            unavailable: false,
            stale: false,
          },
        ],
      };
    });
    const resolve = vi.spyOn(client, 'resolveKnowledgeSource').mockResolvedValue({
      available: true,
      entryId: 'birds',
      location: { providerId: 'local', uri: 'file:///birds.mp3' },
    });
    const write = vi.fn().mockResolvedValue(undefined);
    const oldClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: write },
    });
    try {
      mount({ client, initialSubject: 'retrieval' });
      await ready();
      await search();
      button('Copy details and local source link').click();
      await vi.waitFor(() =>
        expect(write).toHaveBeenCalledWith('Birds & wind\n\nSource: birds.mp3\nfile:///birds.mp3'),
      );
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-knowledge-media-copy [role="status"]')?.textContent).toBe(
          'Details and local source link copied',
        ),
      );
      resolve.mockResolvedValue({
        available: true,
        entryId: 'remote-birds',
        location: { providerId: 'sftp', uri: 'sftp://private/birds.mp3' },
      });
      button('Copy details and local source link').click();
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-knowledge-media-copy [role="alert"]')?.textContent).toBe(
          'A portable source link is unavailable for this remote file.',
        ),
      );
      expect(write).toHaveBeenCalledOnce();
      expect(root.querySelector('.fm-knowledge-media-copy [role="status"]')).toBeNull();
    } finally {
      if (oldClipboard === undefined) Reflect.deleteProperty(navigator, 'clipboard');
      else Object.defineProperty(navigator, 'clipboard', oldClipboard);
    }
  });

  it('lazily resolves authorized image sources and shows a bounded preview in both locales', async () => {
    let intersect: ((entries: IntersectionObserverEntry[]) => void) | undefined;
    const disconnect = vi.fn();
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        constructor(callback: (entries: IntersectionObserverEntry[]) => void) {
          intersect = callback;
        }
        observe(): void {}
        disconnect(): void {
          disconnect();
        }
      },
    );
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            title: 'garden.png',
            mediaType: 'image/png',
            provenance: '',
            sectionPath: [],
            unavailable: false,
            stale: false,
          },
        ],
      };
    });
    const resolve = vi.spyOn(client, 'resolveKnowledgeSource').mockResolvedValue({
      available: true,
      entryId: 'garden',
      location: { providerId: 'local', uri: 'file:///garden.png' },
    });
    const thumbnail = vi
      .spyOn(client, 'getThumbnail')
      .mockResolvedValue(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]));
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    await search();
    expect(resolve).not.toHaveBeenCalled();
    expect(root.querySelector('.fm-knowledge-image-placeholder')?.getAttribute('aria-label')).toBe(
      'Loading image preview',
    );

    intersect?.([{ isIntersecting: true } as IntersectionObserverEntry]);
    await vi.waitFor(() =>
      expect(thumbnail).toHaveBeenCalledWith(
        'file:///garden.png',
        'small',
        expect.any(AbortSignal),
      ),
    );
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLImageElement>('.fm-knowledge-image-preview img')?.alt).toBe(
        'Preview of garden.png',
      ),
    );
    expect(resolve).toHaveBeenCalledWith(
      { workspaceId, sourceId: expect.any(String) },
      expect.any(AbortSignal),
    );
    expect(root.querySelector<HTMLImageElement>('.fm-knowledge-image-preview img')?.src).toContain(
      'data:image/jpeg;base64,',
    );
    m.mount(root, null);
    expect(disconnect).toHaveBeenCalled();

    setLocale('nl');
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    await search();
    intersect?.([{ isIntersecting: true } as IntersectionObserverEntry]);
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLImageElement>('.fm-knowledge-image-preview img')?.alt).toBe(
        'Voorbeeld van garden.png',
      ),
    );
  });

  it('does not substitute a PDF source thumbnail for an indexed image', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            mediaType: 'image/jpeg',
            title: 'scanned.pdf',
            provenance: '',
            sectionPath: [],
            unavailable: false,
            stale: false,
          },
        ],
      };
    });
    vi.spyOn(client, 'resolveKnowledgeSource').mockResolvedValue({
      available: true,
      entryId: 'pdf',
      location: { providerId: 'local', uri: 'file:///scanned.pdf' },
    });
    const thumbnail = vi.spyOn(client, 'getThumbnail');
    mount({ client, initialSubject: 'retrieval' });
    await ready();
    await search();
    await vi.waitFor(() =>
      expect(
        root.querySelector('.fm-knowledge-image-placeholder')?.getAttribute('aria-label'),
      ).toBe('Image preview unavailable'),
    );
    expect(thumbnail).not.toHaveBeenCalled();
  });

  it('reflects the actual image viewer state in the action label and expanded state', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [{ ...first, mediaType: 'image/png', unavailable: false }],
      };
    });
    let openSourceId: string | undefined;
    const onOpenSource = vi.fn((evidence: KnowledgeEvidence) => {
      openSourceId = openSourceId === evidence.sourceId ? undefined : evidence.sourceId;
    });
    mount({
      client,
      initialSubject: 'retrieval',
      onOpenSource,
      isSourceOpen: (evidence) => openSourceId === evidence.sourceId,
    });
    await ready();
    await search();

    const button = root.querySelector<HTMLButtonElement>('.fm-knowledge-media-open');
    expect(button?.textContent).toBe('Open image');
    expect(button?.getAttribute('aria-expanded')).toBe('false');
    button?.focus();
    expect(document.activeElement).toBe(button);
    button?.click();
    await vi.waitFor(() => expect(button?.textContent).toBe('Close image'));
    expect(button?.getAttribute('aria-expanded')).toBe('true');
    button?.click();
    await vi.waitFor(() => expect(button?.textContent).toBe('Open image'));
    expect(button?.getAttribute('aria-expanded')).toBe('false');
    expect(onOpenSource).toHaveBeenCalledTimes(2);

    openSourceId = 'unrelated-source';
    m.redraw.sync();
    expect(button?.textContent).toBe('Open image');
    expect(button?.getAttribute('aria-expanded')).toBe('false');
    setLocale('nl');
    m.redraw.sync();
    expect(button?.textContent).toBe('Open afbeelding');
    button?.click();
    await vi.waitFor(() => expect(button?.textContent).toBe('Sluit afbeelding'));
  });

  it('returns the first image to Open when a different image becomes the active preview', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          { ...first, mediaType: 'image/png', unavailable: false },
          {
            ...first,
            documentId: 'second-image-document',
            sourceId: 'second-image-source',
            recordId: 'second-image-record',
            title: 'second.png',
            finalRank: 2,
            mediaType: 'image/png',
            unavailable: false,
          },
        ],
      };
    });
    let displayedSourceId: string | undefined;
    mount({
      client,
      initialSubject: 'retrieval',
      onOpenSource: (evidence) => {
        displayedSourceId = displayedSourceId === evidence.sourceId ? undefined : evidence.sourceId;
      },
      isSourceOpen: (evidence) => displayedSourceId === evidence.sourceId,
    });
    await ready();
    await search();

    const [first, second] = root.querySelectorAll<HTMLButtonElement>('.fm-knowledge-media-open');
    expect(first?.textContent).toBe('Open image');
    expect(second?.textContent).toBe('Open image');
    first?.focus();
    expect(document.activeElement).toBe(first);
    first?.click();
    await vi.waitFor(() => expect(first?.textContent).toBe('Close image'));
    expect(first?.getAttribute('aria-expanded')).toBe('true');
    second?.click();
    await vi.waitFor(() => expect(second?.textContent).toBe('Close image'));
    expect(first?.textContent).toBe('Open image');
    expect(first?.getAttribute('aria-expanded')).toBe('false');
    expect(second?.getAttribute('aria-expanded')).toBe('true');
    second?.click();
    await vi.waitFor(() => expect(second?.textContent).toBe('Open image'));
    expect(second?.getAttribute('aria-expanded')).toBe('false');
    first?.click();
    await vi.waitFor(() => expect(first?.textContent).toBe('Close image'));
    const firstSourceId = displayedSourceId;
    displayedSourceId = undefined;
    m.redraw.sync();
    expect(first?.textContent).toBe('Open image');
    displayedSourceId = firstSourceId;
    m.redraw.sync();
    expect(first?.textContent).toBe('Close image');
    expect(second?.textContent).toBe('Open image');
  });

  it('does not open unavailable media and never treats media placeholders as markdown', async () => {
    const client = new MockFileManagerClient();
    const original = client.executeKnowledgeSearch.bind(client);
    vi.spyOn(client, 'executeKnowledgeSearch').mockImplementation(async (request, signal) => {
      const result = await original(request, signal);
      const first = result.evidence[0];
      if (first === undefined) throw new Error('missing knowledge fixture');
      return {
        ...result,
        evidence: [
          {
            ...first,
            mediaType: 'audio/wav',
            excerpt: '<|audio|>',
            content: '[audio]',
            unavailable: true,
          },
        ],
      };
    });
    const onOpenSource = vi.fn();
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await ready();
    await search();
    expect(root.querySelector('.fm-knowledge-media-evidence')?.textContent).toContain(
      'Open the source',
    );
    expect(root.querySelector<HTMLButtonElement>('.fm-knowledge-media-open')?.disabled).toBe(true);
    expect(root.querySelector('.fm-knowledge-source-link')?.hasAttribute('disabled')).toBe(true);
    expect(onOpenSource).not.toHaveBeenCalled();
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
        'Semantic search encountered an internal error. Try again; if it continues, rebuild the semantic index.',
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
        'Semantic search is not available right now.',
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
    expect(searchButton().disabled).toBe(true);
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
    expect(searchButton().disabled).toBe(false);

    submitSearch();

    await vi.waitFor(() => expect(execute).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(searchButton().disabled).toBe(false));
    searchButton().click();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(2));
  });

  it('labels every control and announces results politely', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    expect(subjects().getAttribute('aria-describedby')).toBeNull();
    expect(root.querySelector('#fm-knowledge-subjects-hint')).toBeNull();
    expect(root.querySelector('.fm-knowledge-needs legend')?.textContent).toBe('Search for:');
    expect(root.querySelector('.fm-knowledge-modes legend')?.textContent).toBe('Retrieval');
    expect(root.querySelector('.fm-knowledge-results-body')?.getAttribute('aria-live')).toBe(
      'polite',
    );
    for (const mode of root.querySelectorAll('.fm-knowledge-mode')) {
      expect(mode.getAttribute('aria-pressed')).toMatch(/true|false/u);
    }
  });

  it('defaults to enrolled workspace roots even with semantic results available', async () => {
    mount({ semanticSourceIds: ['mock-knowledge-source-onboarding'] });
    await ready();

    expect(root.querySelector<HTMLSelectElement>('#fm-knowledge-scope')?.value).toBe(
      'enrolledRoots',
    );
  });

  it('defaults to enrolled workspace roots without a displayed folder', async () => {
    mount();
    await ready();

    expect(root.querySelector<HTMLSelectElement>('#fm-knowledge-scope')?.value).toBe(
      'enrolledRoots',
    );
    const input = root.querySelector<HTMLTextAreaElement>('#fm-knowledge-subjects');
    expect(input?.placeholder).toBe('Enter your search terms');
    expect(input?.getAttribute('aria-label')).toBeTruthy();
    expect(input?.getAttribute('autocomplete')).toBe('off');
    expect(input?.getAttribute('autocorrect')).toBe('off');
    expect(input?.getAttribute('autocapitalize')).toBe('off');
    expect(input?.getAttribute('spellcheck')).toBeNull();
  });

  it('uses a distinct Dutch search placeholder while keeping a programmatic label', async () => {
    setLocale('nl');
    mount();
    await ready();
    const input = root.querySelector<HTMLTextAreaElement>('#fm-knowledge-subjects');
    expect(input?.placeholder).toBe('Voer je zoektermen in');
    expect(input?.getAttribute('aria-label')).toBeTruthy();
  });

  it('searches enrolled roots across navigation and pins the optional folder filter', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'includedHere',
      rootId: 'mock-knowledge-root-handbook',
      exclusionId: null,
      workspaceReferenced: true,
      sourceAvailable: true,
      unavailableReason: null,
    });

    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    let displayedFolder = { providerId: 'local', uri: 'file:///documents/first' };
    mount({
      client,
      initialSubject: 'retrieval',
      currentFolder: () => displayedFolder,
    });
    await ready();
    const folderFilter = () =>
      root.querySelector<HTMLInputElement>('.fm-knowledge-folder-filter input');
    expect(folderFilter()?.checked).toBe(false);
    expect(folderFilter()?.closest('label')?.textContent).toContain('first');

    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(1));
    expect(execute.mock.lastCall?.[0].scope).toMatchObject({
      workspaceId,
      kind: 'enrolledRoots',
      folder: null,
    });
    await vi.waitFor(() => expect(searchButton().disabled).toBe(false));
    displayedFolder = { providerId: 'local', uri: 'file:///documents/second' };
    m.redraw.sync();
    expect(folderFilter()?.closest('label')?.textContent).toContain('first');
    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(2));
    expect(execute.mock.lastCall?.[0].scope).toMatchObject({
      workspaceId,
      kind: 'enrolledRoots',
      folder: null,
    });
    await vi.waitFor(() => expect(searchButton().disabled).toBe(false));

    folderFilter()?.click();
    m.redraw.sync();
    expect(folderFilter()?.checked).toBe(true);
    expect(root.querySelector('#fm-knowledge-scope')).toBeNull();
    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(3));
    expect(execute.mock.lastCall?.[0].scope).toMatchObject({
      workspaceId,
      kind: 'currentFolder',
      folder: { uri: 'file:///documents/first' },
    });
    await vi.waitFor(() => expect(searchButton().disabled).toBe(false));

    folderFilter()?.click();
    m.redraw.sync();
    expect(folderFilter()?.checked).toBe(false);
    submitSearch();
    await vi.waitFor(() => expect(execute).toHaveBeenCalledTimes(4));
    expect(execute.mock.lastCall?.[0].scope).toMatchObject({
      workspaceId,
      kind: 'enrolledRoots',
      folder: null,
    });
  });

  it.each([
    ['file:///Custom%20Chat%20Leiden', 'Custom Chat Leiden'],
    ['file:///A%2520B%2C%20C', 'A%20B, C'],
    ['file:///A%22B%27', 'A"B\''],
    ['file:///', '/'],
  ])(
    'displays a decoded folder label without changing the pinned URI for %s',
    async (uri, name) => {
      const client = new MockFileManagerClient();
      vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
        consent: 'includedHere',
        rootId: 'mock-knowledge-root-handbook',
        exclusionId: null,
        workspaceReferenced: true,
        sourceAvailable: true,
        unavailableReason: null,
      });
      const execute = vi.spyOn(client, 'executeKnowledgeSearch');
      mount({
        client,
        initialSubject: 'retrieval',
        currentFolder: { providerId: 'local', uri },
      });
      await ready();
      const filter = root.querySelector<HTMLInputElement>('.fm-knowledge-folder-filter input');
      expect(filter?.closest('label')?.textContent).toContain(`"${name}"`);
      filter?.click();
      submitSearch();
      await vi.waitFor(() => expect(execute).toHaveBeenCalledOnce());
      expect(execute.mock.lastCall?.[0].scope).toMatchObject({
        workspaceId,
        kind: 'currentFolder',
        folder: { uri },
      });
    },
  );

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

  it('ranks documents by their best match and sections by relevance', () => {
    const row = (
      documentId: string,
      recordId: string,
      sourcePosition: number,
      finalRank: number,
      unavailable = false,
      adjacent = false,
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
      adjacent,
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
      row('document-a', 'record-a-adjacent', 2, 1, false, true),
    ]);

    expect(groups).toHaveLength(2);
    expect(groups.map((group) => group.documentId)).toEqual(['document-a', 'document-b']);
    expect(groups[0]?.rows.map((entry) => entry.finalRank)).toEqual([1, 4]);
    expect(groups[0]?.rows.map((entry) => entry.recordId)).not.toContain('record-a-adjacent');
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
        currentFolder:
          typeof options.currentFolder === 'function'
            ? options.currentFolder()
            : options.currentFolder,
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
    expect(searchButton().disabled).toBe(true);
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

    await vi.waitFor(() =>
      expect(
        [...root.querySelectorAll<HTMLInputElement>('.fm-knowledge-roots input')].map(
          (input) => input.checked,
        ),
      ).toEqual([true, false]),
    );
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
    await vi.waitFor(() => expect(dsl().value).toContain('scope: library'));
    expect(scopeSelect().value).toBe('enrolledRoots');

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
    await search();
    expect(execute).toHaveBeenCalledOnce();
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
    await vi.waitFor(() => expect(root.textContent).toContain('Search for:'));

    expect(root.querySelector('.fm-knowledge-generate')).toBeNull();
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

function question(): HTMLTextAreaElement {
  const field = root.querySelector<HTMLTextAreaElement>('#fm-knowledge-question');
  if (field === null) throw new Error('separate answer question not rendered');
  return field;
}

function openAsk(askedQuestion = 'How does this work?'): void {
  const field = question();
  if (field.value === '') type(field, askedQuestion);
}

function modelKnowledgeCheckbox(): HTMLInputElement {
  const element = root.querySelector<HTMLInputElement>('#fm-knowledge-model-knowledge');
  if (element === null) throw new Error('model knowledge checkbox not rendered');
  return element;
}

/** Waits until an answer section exists after a completed search. */
async function generateAnswer(): Promise<void> {
  button('Ask').click();
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
/** Waits until profiles have loaded and the docked ask bar is rendered. */
async function askBarReady(): Promise<void> {
  await vi.waitFor(() => expect(root.querySelector('#fm-knowledge-question')).not.toBeNull());
  m.redraw.sync();
}

async function answersReady(): Promise<void> {
  await vi.waitFor(() => {
    expect(root.textContent).not.toContain('Loading knowledge search…');
    expect(subjects().disabled).toBe(false);
  });
  m.redraw.sync();
}

describe('KnowledgeSearchDialog optional answers (task 0207)', () => {
  it('offers Ask for an activated Ollama profile without reopening an existing Knowledge tab', async () => {
    const client = new MockFileManagerClient();
    let revision = 0;
    mount({ client, initialSubject: 'retrieval', profileRevision: () => revision });
    await ready();
    await search();
    expect(answerSection()).toBeNull();

    const profile = await client.createLlmProfile({
      name: 'Local Ollama',
      preset: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
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
    await client.activateLlmProfile(profile.id, false);
    revision++;
    m.redraw.sync();

    await askBarReady();
    expect(root.querySelector('.fm-knowledge-ask-bar')).not.toBeNull();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('hides Ask if refreshed host capabilities revoke generation without losing search results', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    let revision = 0;
    mount({ client, initialSubject: 'retrieval', profileRevision: () => revision });
    await askBarReady();
    await search();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: true,
      semantic: false,
      answerGeneration: false,
    });
    revision++;
    m.redraw.sync();

    await vi.waitFor(() => expect(answerSection()).toBeNull());
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('retrieves once and answers only after an explicit question', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();

    await search();
    expect(execute).toHaveBeenCalledOnce();
    expect(generate).not.toHaveBeenCalled();
    openAsk('What do these sources say?');
    openAsk();
    await generateAnswer();
    expect(generate).toHaveBeenCalledOnce();
    expect(execute).toHaveBeenCalledOnce();
    expect(generate.mock.calls[0]?.[0]?.question).toBe('What do these sources say?');
    expect(root.querySelector('.fm-knowledge-answer-markdown')?.textContent).toContain(
      'Answered from',
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('never offers answer controls when the host reports no answer capability', async () => {
    mount({ initialSubject: 'retrieval' });
    await ready();

    await search();

    expect(answerSection()).toBeNull();
    expect(root.querySelector('.fm-knowledge-generate')).toBeNull();
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

  it('hides the ask bar when the host offers answers but no profile is saved', async () => {
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

    expect(answerSection()).toBeNull();
    expect(root.querySelector('#fm-knowledge-question')).toBeNull();
    expect(() => button('Ask')).toThrow();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('explains a missing saved Ollama profile in the docked Ask bar and opens Settings', async () => {
    const client = new MockFileManagerClient();
    const configure = vi.fn();
    mount({ client, initialSubject: 'retrieval', onConfigureAnswers: configure });
    await ready();
    await search();

    const bar = root.querySelector('.fm-knowledge-ask-unavailable');
    expect(bar?.textContent).toContain(
      'To use Ask, choose a model and save a generation profile in Settings.',
    );
    expect(root.querySelector('#fm-knowledge-question')).toBeNull();
    bar?.querySelector<HTMLButtonElement>('.fm-knowledge-configure-answers')?.click();
    expect(configure).toHaveBeenCalledOnce();

    setLocale('nl');
    m.redraw.sync();
    expect(bar?.textContent).toContain(
      'Kies een model en sla een generatieprofiel op in Instellingen om Ask te gebruiken.',
    );
  });

  it('searches with the question first when asked before any search', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client });
    await answersReady();
    await askBarReady();

    openAsk();
    type(question(), 'How does retrieval work?');
    await generateAnswer();

    expect(execute).toHaveBeenCalledOnce();
    expect(execute.mock.calls[0]?.[0]?.draft.about).toEqual(['How does retrieval work?']);
    expect(execute.mock.calls[0]?.[0]?.intent).toBe('questionAnswering');
    expect(subjects().value).toContain('How does retrieval work?');
    expect(generate.mock.calls[0]?.[0]?.question).toBe('How does retrieval work?');
    expect(question().value).toBe('How does retrieval work?');
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('searches the entered subjects, not the question, when they were not yet searched', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await askBarReady();

    openAsk();
    type(question(), 'How is it ranked?');
    await generateAnswer();

    expect(execute).toHaveBeenCalledOnce();
    expect(execute.mock.calls[0]?.[0]?.draft.about).toEqual(['retrieval']);
    expect(execute.mock.calls[0]?.[0]?.intent).toBe('questionAnswering');
    expect(generate.mock.calls[0]?.[0]?.question).toBe('How is it ranked?');
  });

  it('answers with the default profile activated in Settings', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client, { name: 'First profile' });
    const chosen = await configureProfile(client, { name: 'Chosen profile' });
    await client.activateLlmProfile(chosen.id, false);
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();

    expect(root.querySelector('#fm-knowledge-answer-profile')).toBeNull();
    expect(button('Ask').disabled).toBe(true);
    openAsk();
    expect(button('Ask').disabled).toBe(false);
    await generateAnswer();

    expect(generate.mock.calls[0]?.[0]?.profileId).toBe(chosen.id);
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
    openAsk();

    await generateAnswer();

    expect(execute.mock.calls.length).toBe(searches);
    expect(generate).toHaveBeenCalledOnce();
    const request = generate.mock.calls[0]?.[0];
    expect(request?.evidenceFingerprint).toBe(displayedFingerprint);
    expect(request?.question).toBe('How does this work?');
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

  it('keeps the previous answer while typing and replaces it once the next question is asked', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();

    expect(subjects().value).toBe('retrieval');
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
    openAsk();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).not.toBeNull();
    type(question(), 'What changed?');
    expect(root.querySelector('.fm-knowledge-answer-markdown')).not.toBeNull();
    await generateAnswer();

    expect(execute).toHaveBeenCalledOnce();
    expect(generate).toHaveBeenCalledTimes(2);
    expect(generate.mock.calls[1]?.[0]?.question).toBe('What changed?');
  });

  it('carries answer-only constraints without adding them to retrieval', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
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
    openAsk();

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
    await configureProfile(client);
    const generate = vi.spyOn(client, 'generateKnowledgeAnswer');
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();

    expect(modelKnowledgeCheckbox().checked).toBe(false);
    expect(
      modelKnowledgeCheckbox().closest('[data-tooltip]')?.getAttribute('data-tooltip'),
    ).toContain('clearly labelled');
    modelKnowledgeCheckbox().click();
    m.redraw.sync();
    expect(modelKnowledgeCheckbox().checked).toBe(true);
    expect(root.querySelector('.fm-knowledge-model-knowledge.is-active')).not.toBeNull();

    await generateAnswer();

    expect(generate.mock.calls[0]?.[0]?.allowModelKnowledge).toBe(true);
    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'General model knowledge was allowed',
    );
  });

  it('discloses a cloud endpoint before any evidence is sent', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client, { name: 'Local profile', baseUrl: 'http://localhost:11434' });
    const cloud = await configureProfile(client, {
      name: 'Cloud profile',
      baseUrl: 'https://llm.example.test',
    });
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await askBarReady();
    expect(root.querySelector('.fm-knowledge-ask-cloud')).toBeNull();

    await client.activateLlmProfile(cloud.id, true);
    m.mount(root, null);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await askBarReady();
    const cloudHint = root.querySelector('.fm-knowledge-ask-cloud')?.closest('[data-tooltip]');
    expect(cloudHint?.getAttribute('data-tooltip')).toContain('sends the inspected evidence');
  });

  it('tells the user to search again when the inspected evidence is gone', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');
    vi.spyOn(client, 'generateKnowledgeAnswer').mockRejectedValue(
      Object.assign(new Error('gone'), { code: 'knowledgeEvidenceRefreshRequired' }),
    );
    openAsk();

    button('Ask').click();

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
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(
      (_request, signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener('abort', () =>
            reject(new DOMException('The operation was aborted.', 'AbortError')),
          );
        }),
    );

    button('Ask').click();
    m.redraw.sync();
    expect(root.textContent).toContain('Generating answer…');
    button('Cancel answer').click();

    await vi.waitFor(() => expect(root.textContent).toContain('The answer was cancelled.'));
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('reports a generation failure without losing the results', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockRejectedValue(new Error('boom'));
    openAsk();

    button('Ask').click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-knowledge-answer [role="alert"]')?.textContent).toContain(
        'The answer could not be generated.',
      ),
    );
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('hides the ask bar when profiles fail to load and keeps search complete', async () => {
    const client = new MockFileManagerClient({
      failures: { listLlmProfiles: new Error('offline') },
    });
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();

    await search();

    expect(answerSection()).toBeNull();
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
    expect(root.querySelector('#fm-knowledge-answer-profile')).toBeNull();
  });

  it('clears a generated answer as soon as the query is edited', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).not.toBeNull();

    type(subjects(), 'fusion');

    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('clears a generated answer when a new search is started', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();

    button('Search').click();
    m.redraw.sync();
    await search();

    openAsk();
    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
  });

  it('discards an answer that lands after a further edit', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const pending = deferred<KnowledgeAnswer>();
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(() => pending.promise);

    button('Ask').click();
    m.redraw.sync();
    button('Search').click();
    m.redraw.sync();
    type(subjects(), 'fusion');
    pending.resolve(answerFixture('stale-fingerprint'));
    await Promise.resolve();
    m.redraw.sync();

    expect(root.querySelector('.fm-knowledge-answer-markdown')).toBeNull();
    expect(root.textContent).not.toContain('Grounded answer');
  });

  it('keeps an in-flight answer while the next question is typed', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const pending = deferred<KnowledgeAnswer>();
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    let signal: AbortSignal | undefined;
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation((_request, current) => {
      signal = current;
      return pending.promise;
    });
    button('Ask').click();
    m.redraw.sync();
    type(question(), 'Another question?');
    expect(signal?.aborted).toBe(false);
    expect(root.querySelectorAll('.fm-knowledge-result').length).toBeGreaterThan(0);
  });

  it('discards an answer that lands after the dialog was closed and reopened', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const pending = deferred<KnowledgeAnswer>();
    const lifecycle = mountLifecycle({ client, initialSubject: 'retrieval' });
    lifecycle.open(true);
    await answersReady();
    await search();
    openAsk();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(() => pending.promise);
    button('Ask').click();
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
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();

    const citation = root.querySelector<HTMLButtonElement>(
      '.fm-knowledge-citations button:not(:disabled)',
    );
    citation?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.stringContaining('mock-knowledge-source-') }),
    );
  });

  it('numbers sources by file, collapses their list, and hides, shows, or copies the answer', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const write = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: write },
    });
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();

    const markdown = root.querySelector('.fm-knowledge-answer-markdown');
    expect(markdown?.textContent).not.toMatch(/\bE\d+\b/u);
    expect(root.textContent).not.toContain('Answered by');
    const references = root.querySelector<HTMLDetailsElement>(
      'details.fm-knowledge-answer-references',
    );
    expect(references?.open).toBe(false);
    expect(references?.querySelector('summary')?.textContent).toMatch(/^Sources \(\d+\)$/u);
    expect(references?.querySelector('.fm-knowledge-reference-number')?.textContent).toBe('[1]');

    const body = root.querySelector<HTMLElement>('.fm-knowledge-answer-body');
    expect(button('Hide answer').getAttribute('aria-expanded')).toBe('true');
    expect(button('Hide answer').querySelector('.fm-icon-chevron-up')).not.toBeNull();
    button('Hide answer').click();
    m.redraw.sync();
    expect(body?.hidden).toBe(true);
    expect(button('Show answer').getAttribute('aria-expanded')).toBe('false');
    expect(button('Show answer').querySelector('.fm-icon-chevron-down')).not.toBeNull();
    button('Show answer').click();
    m.redraw.sync();
    expect(body?.hidden).toBe(false);

    button('Copy answer').click();
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(button('Copied').querySelector('.fm-icon-check')).not.toBeNull());
    expect(root.querySelector('.fm-knowledge-answer [role="status"]')?.textContent).toBe('Copied');
    expect(write.mock.calls[0]?.[0]).toMatch(/\n\nSources \(\d+\)\n\[1\] /u);
  });

  it('does not claim an answer was copied when both clipboard methods fail', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error('clipboard denied')) },
    });
    const previousCopyCommand = Object.getOwnPropertyDescriptor(document, 'execCommand');
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      value: vi.fn(() => false),
    });
    try {
      mount({ client, initialSubject: 'retrieval' });
      await answersReady();
      await search();
      openAsk();
      await generateAnswer();

      button('Copy answer').click();
      await vi.waitFor(() =>
        expect(root.querySelector('.fm-knowledge-answer [role="alert"]')?.textContent).toContain(
          'The answer could not be copied.',
        ),
      );
      expect(button('Copy answer').querySelector('.fm-icon-copy')).not.toBeNull();
      expect(root.querySelector('.fm-knowledge-answer .fm-icon-check')).toBeNull();
    } finally {
      if (previousCopyCommand === undefined) Reflect.deleteProperty(document, 'execCommand');
      else Object.defineProperty(document, 'execCommand', previousCopyCommand);
    }
  });

  it('opens a citation from the answer text itself', async () => {
    const onOpenSource = vi.fn();
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval', onOpenSource });
    await answersReady();
    await search();
    openAsk();
    await generateAnswer();

    const link = root.querySelector<HTMLAnchorElement>('.fm-knowledge-answer-markdown a');
    expect(link?.getAttribute('href')).toBe('#fm-knowledge-reference-1');
    expect(link?.closest('.fm-knowledge-reference')?.textContent).toMatch(/^\[1(, \d+)*\]$/u);
    link?.click();

    expect(onOpenSource).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: expect.stringContaining('mock-knowledge-source-') }),
    );
  });

  it('never links a citation whose identity was not displayed', async () => {
    const onOpenSource = vi.fn();
    const client = new MockFileManagerClient();
    await configureProfile(client);
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
    openAsk();

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer-markdown a')).toBeNull();
    const citation = root.querySelector<HTMLButtonElement>('.fm-knowledge-citations button');
    expect(citation?.disabled).toBe(true);
    citation?.click();
    expect(onOpenSource).not.toHaveBeenCalled();
  });

  it('renders answer markdown safely', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const original = client.generateKnowledgeAnswer.bind(client);
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => ({
      ...(await original(request, signal)),
      text: '# Heading\n\n<script>window.pwned = true;</script>\n\n**bold**',
    }));
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer-markdown h1')?.textContent).toBe('Heading');
    expect(root.querySelector('.fm-knowledge-answer-markdown strong')?.textContent).toBe('bold');
    expect(root.querySelector('.fm-knowledge-answer-markdown script')).toBeNull();
  });

  it('reports stale, unavailable, generated and withheld provenance honestly', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
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
    openAsk();

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
    await configureProfile(client);
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
    openAsk();

    await generateAnswer();

    expect(root.querySelector('.fm-knowledge-answer')?.textContent).toContain(
      'could not support a grounded answer',
    );
  });

  it('labels every answer control and announces generation politely', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();

    openAsk();
    expect(answerSection()?.getAttribute('aria-label')).toBe('Optional knowledge answer');
    expect(root.querySelector('label[for="fm-knowledge-question"]')?.textContent).toBe(
      'Question about these results',
    );
    expect(root.querySelector('label[for="fm-knowledge-model-knowledge"]')?.textContent).toContain(
      'general model knowledge',
    );
    expect(root.querySelector('.fm-knowledge-answer-body')?.getAttribute('aria-live')).toBe(
      'polite',
    );

    openAsk();
    question().focus();
    await generateAnswer();

    expect(document.activeElement).toBe(question());
    expect(root.querySelector('.fm-knowledge-answer-body')?.hasAttribute('tabindex')).toBe(false);
  });

  it('shows an accessible answer skeleton only while generating', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    mount({ client, initialSubject: 'retrieval' });
    await answersReady();
    await search();
    openAsk();
    const original = client.generateKnowledgeAnswer.bind(client);
    let finish: (() => void) | undefined;
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(async (request, signal) => {
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
      return original(request, signal);
    });

    button('Ask').click();
    m.redraw.sync();
    expect(answerSection()?.getAttribute('aria-busy')).toBe('true');
    const progress = root.querySelector('.fm-knowledge-answer-progress');
    expect(progress?.getAttribute('role')).toBe('status');
    expect(progress?.textContent).toContain('Generating');
    expect(progress?.querySelectorAll('.fm-knowledge-answer-skeleton .mm-skeleton')).toHaveLength(
      3,
    );
    expect(
      progress?.querySelector('.fm-knowledge-answer-skeleton')?.getAttribute('aria-hidden'),
    ).toBe('true');
    expect(progress?.querySelector('.fm-knowledge-search-spinner')).toBeNull();
    expect(root.querySelector('.fm-knowledge-ask-cancel')).not.toBeNull();

    finish?.();
    await vi.waitFor(() => expect(answerSection()?.getAttribute('aria-busy')).toBe('false'));
    expect(root.querySelector('.fm-knowledge-answer-progress')).toBeNull();
    expect(root.querySelector('.fm-knowledge-ask-cancel')).toBeNull();
  });

  it('cancels a running generation when the dialog is closed', async () => {
    const client = new MockFileManagerClient();
    await configureProfile(client);
    const onClose = vi.fn();
    let aborted = false;
    mount({ client, initialSubject: 'retrieval', onClose });
    await answersReady();
    await search();
    openAsk();
    vi.spyOn(client, 'generateKnowledgeAnswer').mockImplementation(
      (_request, signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener('abort', () => {
            aborted = true;
            reject(new DOMException('The operation was aborted.', 'AbortError'));
          });
        }),
    );

    button('Ask').click();
    m.redraw.sync();
    subjects().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));

    expect(onClose).toHaveBeenCalledOnce();
    await vi.waitFor(() => expect(aborted).toBe(true));
  });
});
