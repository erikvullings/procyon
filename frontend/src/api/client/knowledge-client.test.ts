import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  ExecuteKnowledgeSearchRequest,
  KnowledgeCapabilities,
  KnowledgeQueryInterpretation,
  KnowledgeRoot,
  KnowledgeSearchPlan,
  KnowledgeSearchResult,
  KnowledgeSourceLocation,
} from '../../models';

const getKnowledgeCapabilities = vi.fn();
const listKnowledgeRoots = vi.fn();
const parseKnowledgeQuery = vi.fn();
const planKnowledgeSearch = vi.fn();
const executeKnowledgeSearch = vi.fn();
const cancelKnowledgeSearch = vi.fn();
const resolveKnowledgeSource = vi.fn();

vi.mock('../generated/file-manager-api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../generated/file-manager-api')>();
  return {
    ...actual,
    getKnowledgeCapabilities: (...args: unknown[]) => getKnowledgeCapabilities(...args),
    listKnowledgeRoots: (...args: unknown[]) => listKnowledgeRoots(...args),
    parseKnowledgeQuery: (...args: unknown[]) => parseKnowledgeQuery(...args),
    planKnowledgeSearch: (...args: unknown[]) => planKnowledgeSearch(...args),
    executeKnowledgeSearch: (...args: unknown[]) => executeKnowledgeSearch(...args),
    cancelKnowledgeSearch: (...args: unknown[]) => cancelKnowledgeSearch(...args),
    resolveKnowledgeSource: (...args: unknown[]) => resolveKnowledgeSource(...args),
  };
});

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    constructor(public onmessage: (message: unknown) => void) {}
  },
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onDragDropEvent: vi.fn() }),
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

const { HttpFileManagerClient } = await import('./http-file-manager-client');
const { MockFileManagerClient } = await import('./mock-file-manager-client');
const { TauriFileManagerClient } = await import('./tauri-file-manager-client');
const {
  KNOWLEDGE_PARSE_CONFORMANCE_CASES,
  KNOWLEDGE_WORKSPACE_ID,
  knowledgeDraft,
  knowledgeOptions,
  knowledgeScope,
} = await import('./knowledge-search-fixtures');

const workspaceId = KNOWLEDGE_WORKSPACE_ID;

class TestEventSource extends EventTarget {
  close(): void {}
}

beforeEach(() => {
  vi.stubGlobal('EventSource', TestEventSource);
});

afterEach(() => {
  getKnowledgeCapabilities.mockReset();
  listKnowledgeRoots.mockReset();
  parseKnowledgeQuery.mockReset();
  planKnowledgeSearch.mockReset();
  executeKnowledgeSearch.mockReset();
  cancelKnowledgeSearch.mockReset();
  resolveKnowledgeSource.mockReset();
  vi.unstubAllGlobals();
});

/** Every knowledge method a host adapter must implement (task 0206). */
const KNOWLEDGE_METHODS = [
  'getKnowledgeCapabilities',
  'listKnowledgeRoots',
  'parseKnowledgeQuery',
  'planKnowledgeSearch',
  'executeKnowledgeSearch',
  'cancelKnowledgeSearch',
  'resolveKnowledgeSource',
] as const;

function capabilitiesFixture(): KnowledgeCapabilities {
  return { fullText: true, semantic: false, answerGeneration: false };
}

function planFixture(): KnowledgeSearchPlan {
  return {
    version: 'structured-knowledge-planner/1',
    subjects: ['retrieval'],
    scope: {
      workspaceId,
      kind: 'entireLibrary',
      folder: null,
      enrolledRootIds: [],
      semanticSourceIds: [],
    },
    scopeLabel: 'Entire indexed library',
    scopeIsExact: true,
    authorizedSources: 3,
    mode: 'hybrid',
    options: {
      maximumSearches: 8,
      candidateLimit: 64,
      resultLimit: 20,
      maximumResultsPerFile: 3,
      contextTokenBudget: 8_192,
      adjacentChunkRadius: 1,
      sectionBoundedContext: true,
      includeTrace: false,
    },
    searches: [
      {
        text: 'retrieval',
        priority: 'primary',
        reasons: [
          { kind: 'subject', subjectIndex: 0, need: null, action: null, relatedTermIndex: null },
        ],
      },
    ],
    omittedSearches: 0,
    excludedFromRetrieval: [],
  };
}

function resultFixture(requestId: string): KnowledgeSearchResult {
  return {
    requestId,
    plan: planFixture(),
    capabilities: capabilitiesFixture(),
    route: {
      requested: 'hybrid',
      applied: 'fullText',
      fallbackReason: 'queryEmbeddingsUnavailable',
    },
    evidence: [],
    evidenceFingerprint: 'fingerprint',
    tokenCount: 0,
    withheldUnauthorized: 0,
    coverage: {
      eligible: 3,
      indexed: 3,
      fingerprinted: 3,
      partial: false,
      scopeIsExact: true,
      staleEvidence: 0,
      unavailable: 0,
      unavailableEvidence: 0,
      unknownFreshnessEvidence: 0,
    },
    trace: null,
  };
}

function executeRequest(requestId: string): ExecuteKnowledgeSearchRequest {
  return {
    requestId,
    draft: knowledgeDraft({ about: ['retrieval'] }),
    scope: knowledgeScope(),
    mode: 'hybrid',
    options: null,
  };
}

describe('knowledge search host parity (task 0206)', () => {
  it('implements every knowledge method on all three adapters', () => {
    for (const method of KNOWLEDGE_METHODS) {
      expect(typeof HttpFileManagerClient.prototype[method], `http ${method}`).toBe('function');
      expect(typeof MockFileManagerClient.prototype[method], `mock ${method}`).toBe('function');
      expect(typeof TauriFileManagerClient.prototype[method], `tauri ${method}`).toBe('function');
    }
  });
});

describe('HttpFileManagerClient knowledge search', () => {
  it('reports capabilities and roots through the generated client', async () => {
    getKnowledgeCapabilities.mockResolvedValue({ status: 200, data: capabilitiesFixture() });
    const roots: KnowledgeRoot[] = [
      {
        rootId: 'root-a',
        label: 'Handbook',
        location: { providerId: 'file', uri: 'file:///handbook' },
        recursive: true,
        indexedGeneration: 3,
        available: true,
      },
    ];
    listKnowledgeRoots.mockResolvedValue({ status: 200, data: roots });
    const client = new HttpFileManagerClient();

    await expect(client.getKnowledgeCapabilities()).resolves.toEqual(capabilitiesFixture());
    await expect(client.listKnowledgeRoots({ workspaceId })).resolves.toEqual(roots);
    expect(listKnowledgeRoots).toHaveBeenCalledWith({ workspaceId }, undefined);
  });

  it('parses and plans without contacting a model', async () => {
    const interpretation: KnowledgeQueryInterpretation = {
      draft: { about: ['retrieval'], needs: ['definition'], related: [], scopes: [] },
      dslCompact: 'about: retrieval need: definition',
      dslMultiline: 'about: retrieval\nneed: definition',
      diagnostics: [],
      ambiguities: [],
      confidence: 'explicit',
      excludedFromRetrieval: [],
    };
    parseKnowledgeQuery.mockResolvedValue({ status: 200, data: interpretation });
    planKnowledgeSearch.mockResolvedValue({ status: 200, data: planFixture() });
    const client = new HttpFileManagerClient();

    await expect(
      client.parseKnowledgeQuery({ text: 'about: retrieval need: definition' }),
    ).resolves.toEqual(interpretation);
    await expect(
      client.planKnowledgeSearch({
        draft: interpretation.draft,
        scope: planFixture().scope,
        mode: 'hybrid',
        options: null,
      }),
    ).resolves.toEqual(planFixture());
  });

  it('passes the abort signal and cancels the same request id on abort', async () => {
    const controller = new AbortController();
    let rejectExecution: ((reason: Error) => void) | undefined;
    executeKnowledgeSearch.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectExecution = reject;
        }),
    );
    cancelKnowledgeSearch.mockImplementation(() => {
      rejectExecution?.(new DOMException('aborted', 'AbortError'));
      return Promise.resolve({ status: 204, data: undefined });
    });
    const client = new HttpFileManagerClient();

    const pending = client.executeKnowledgeSearch(executeRequest('request-a'), controller.signal);
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(executeKnowledgeSearch).toHaveBeenCalledWith(executeRequest('request-a'), {
      signal: controller.signal,
    });
    expect(cancelKnowledgeSearch).toHaveBeenCalledWith({ requestId: 'request-a' }, undefined);
  });

  it('does not cancel a search that completed normally', async () => {
    executeKnowledgeSearch.mockResolvedValue({ status: 200, data: resultFixture('request-b') });
    const controller = new AbortController();
    const client = new HttpFileManagerClient();

    await expect(
      client.executeKnowledgeSearch(executeRequest('request-b'), controller.signal),
    ).resolves.toMatchObject({ requestId: 'request-b' });
    controller.abort();

    expect(cancelKnowledgeSearch).not.toHaveBeenCalled();
  });

  it('resolves an evidence source into a navigable location', async () => {
    const location: KnowledgeSourceLocation = {
      entryId: 'entry-a',
      location: { providerId: 'file', uri: 'file:///handbook/onboarding.md' },
      available: true,
    };
    resolveKnowledgeSource.mockResolvedValue({ status: 200, data: location });
    const client = new HttpFileManagerClient();

    await expect(
      client.resolveKnowledgeSource({ workspaceId, sourceId: 'source-a' }),
    ).resolves.toEqual(location);
  });

  it('rejects an unexpected cancellation status rather than reporting success', async () => {
    cancelKnowledgeSearch.mockResolvedValue({ status: 200, data: undefined });
    const client = new HttpFileManagerClient();

    await expect(client.cancelKnowledgeSearch({ requestId: 'request-c' })).rejects.toThrow(
      /Unexpected cancelKnowledgeSearch response status/u,
    );
  });
});

describe('knowledge search request conformance (task 0206)', () => {
  it('sends the same execute request shape from the HTTP and desktop adapters', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invoked = vi.mocked(invoke);
    invoked.mockReset();
    invoked.mockResolvedValue(resultFixture('request-parity'));
    executeKnowledgeSearch.mockResolvedValue({
      status: 200,
      data: resultFixture('request-parity'),
    });
    const request: ExecuteKnowledgeSearchRequest = {
      requestId: 'request-parity',
      draft: knowledgeDraft({
        about: ['ACME, Inc.'],
        needs: ['definition'],
        related: ['bm25, dense'],
        scopes: [{ kind: 'root', id: 'root-a' }],
      }),
      scope: knowledgeScope({ kind: 'enrolledRoots', enrolledRootIds: ['root-a'] }),
      mode: 'fullText',
      options: knowledgeOptions(true),
    };

    await new HttpFileManagerClient().executeKnowledgeSearch(request);
    await new TauriFileManagerClient().executeKnowledgeSearch(request);

    expect(executeKnowledgeSearch).toHaveBeenCalledWith(request, undefined);
    expect(invoked).toHaveBeenCalledWith('execute_knowledge_search', { request });
  });

  it('forwards every documented query-language case to the host verbatim', async () => {
    const http = new HttpFileManagerClient();
    for (const parseCase of KNOWLEDGE_PARSE_CONFORMANCE_CASES) {
      parseKnowledgeQuery.mockResolvedValue({
        status: 200,
        data: {
          draft: knowledgeDraft(parseCase.draft),
          dslCompact: '',
          dslMultiline: '',
          diagnostics: [],
          ambiguities: [],
          confidence: 'explicit',
          excludedFromRetrieval: [],
        },
      });

      await http.parseKnowledgeQuery({ text: parseCase.text });

      expect(parseKnowledgeQuery, parseCase.name).toHaveBeenLastCalledWith(
        { text: parseCase.text },
        undefined,
      );
    }
  });

  it('interprets every documented query-language case identically in the mock host', async () => {
    const mock = new MockFileManagerClient();

    for (const parseCase of KNOWLEDGE_PARSE_CONFORMANCE_CASES) {
      const parsed = await mock.parseKnowledgeQuery({ text: parseCase.text });

      for (const [field, expected] of Object.entries(parseCase.draft)) {
        expect(
          parsed.draft[field as keyof typeof parsed.draft],
          `${parseCase.name}: ${field}`,
        ).toEqual(expected);
      }
    }
  });
});
