import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  ExecuteKnowledgeSearchRequest,
  GenerateKnowledgeAnswerRequest,
  KnowledgeAnswer,
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
const generateKnowledgeAnswer = vi.fn();
const cancelKnowledgeAnswer = vi.fn();

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
    generateKnowledgeAnswer: (...args: unknown[]) => generateKnowledgeAnswer(...args),
    cancelKnowledgeAnswer: (...args: unknown[]) => cancelKnowledgeAnswer(...args),
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
  generateKnowledgeAnswer.mockReset();
  cancelKnowledgeAnswer.mockReset();
  vi.unstubAllGlobals();
});

/** Every knowledge method a host adapter must implement (tasks 0206, 0207). */
const KNOWLEDGE_METHODS = [
  'getKnowledgeCapabilities',
  'listKnowledgeRoots',
  'parseKnowledgeQuery',
  'planKnowledgeSearch',
  'executeKnowledgeSearch',
  'cancelKnowledgeSearch',
  'resolveKnowledgeSource',
  'generateKnowledgeAnswer',
  'cancelKnowledgeAnswer',
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

function answerRequest(requestId: string): GenerateKnowledgeAnswerRequest {
  return {
    requestId,
    workspaceId,
    evidenceFingerprint: 'fingerprint',
    profileId: 'profile-a',
    allowModelKnowledge: false,
    action: 'explain',
    context: 'onboarding a new maintainer',
    constraints: ['cite every claim'],
    depth: 'brief',
    output: 'bullets',
  };
}

function answerFixture(requestId: string): KnowledgeAnswer {
  return {
    requestId,
    evidenceFingerprint: 'fingerprint',
    profileId: 'profile-a',
    profileName: 'Local profile',
    locality: 'loopback',
    text: 'Grounded in the inspected evidence [E1].',
    citations: [
      {
        label: 'E1',
        recordId: 'record-a',
        sourceId: 'source-a',
        provenance: '',
        sectionPath: ['Overview'],
        finalRank: 1,
        generated: false,
        stale: false,
        unavailable: false,
      },
    ],
    modelKnowledgeAllowed: false,
    insufficient: false,
    withheldUnauthorized: 0,
    staleEvidence: 0,
    unavailableEvidence: 0,
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

/** A saved generation profile the mock host can answer with. */
async function configureMockProfile(client: InstanceType<typeof MockFileManagerClient>): Promise<{
  readonly id: string;
  readonly name: string;
}> {
  const profile = await client.createLlmProfile({
    name: 'Local mock profile',
    preset: 'openAiCompatible',
    baseUrl: 'http://localhost:11434',
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

/** Runs one mock search and returns its displayed result. */
async function mockSearch(
  client: InstanceType<typeof MockFileManagerClient>,
  requestId = 'mock-search',
): Promise<KnowledgeSearchResult> {
  return client.executeKnowledgeSearch({
    requestId,
    draft: knowledgeDraft({ about: ['retrieval'] }),
    scope: knowledgeScope(),
    mode: 'fullText',
    options: knowledgeOptions(),
  });
}

describe('HttpFileManagerClient knowledge answers (task 0207)', () => {
  it('generates an answer from an inspected evidence set', async () => {
    generateKnowledgeAnswer.mockResolvedValue({ status: 200, data: answerFixture('answer-a') });
    const client = new HttpFileManagerClient();

    await expect(client.generateKnowledgeAnswer(answerRequest('answer-a'))).resolves.toEqual(
      answerFixture('answer-a'),
    );
    expect(generateKnowledgeAnswer).toHaveBeenCalledWith(answerRequest('answer-a'), undefined);
    expect(executeKnowledgeSearch).not.toHaveBeenCalled();
    expect(planKnowledgeSearch).not.toHaveBeenCalled();
    expect(parseKnowledgeQuery).not.toHaveBeenCalled();
  });

  it('passes the abort signal and cancels the same request id on abort', async () => {
    const controller = new AbortController();
    let rejectGeneration: ((reason: Error) => void) | undefined;
    generateKnowledgeAnswer.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          rejectGeneration = reject;
        }),
    );
    cancelKnowledgeAnswer.mockImplementation(() => {
      rejectGeneration?.(new DOMException('aborted', 'AbortError'));
      return Promise.resolve({ status: 204, data: undefined });
    });
    const client = new HttpFileManagerClient();

    const pending = client.generateKnowledgeAnswer(answerRequest('answer-b'), controller.signal);
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(generateKnowledgeAnswer).toHaveBeenCalledWith(answerRequest('answer-b'), {
      signal: controller.signal,
    });
    expect(cancelKnowledgeAnswer).toHaveBeenCalledWith({ requestId: 'answer-b' }, undefined);
  });

  it('reports an abort that lost the race against a completed generation', async () => {
    const controller = new AbortController();
    generateKnowledgeAnswer.mockImplementation(() => {
      controller.abort();
      return Promise.resolve({ status: 200, data: answerFixture('answer-race') });
    });
    cancelKnowledgeAnswer.mockResolvedValue({ status: 204, data: undefined });
    const client = new HttpFileManagerClient();

    await expect(
      client.generateKnowledgeAnswer(answerRequest('answer-race'), controller.signal),
    ).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('does not cancel a generation that completed normally', async () => {
    generateKnowledgeAnswer.mockResolvedValue({ status: 200, data: answerFixture('answer-c') });
    const controller = new AbortController();
    const client = new HttpFileManagerClient();

    await expect(
      client.generateKnowledgeAnswer(answerRequest('answer-c'), controller.signal),
    ).resolves.toMatchObject({ requestId: 'answer-c' });
    controller.abort();

    expect(cancelKnowledgeAnswer).not.toHaveBeenCalled();
  });

  it('rejects an unexpected cancellation status rather than reporting success', async () => {
    cancelKnowledgeAnswer.mockResolvedValue({ status: 200, data: undefined });
    const client = new HttpFileManagerClient();

    await expect(client.cancelKnowledgeAnswer({ requestId: 'answer-d' })).rejects.toThrow(
      /Unexpected cancelKnowledgeAnswer response status/u,
    );
  });

  it('propagates a typed refresh-required conflict unchanged', async () => {
    const { ApiError } = await import('../fetch-mutator');
    generateKnowledgeAnswer.mockRejectedValue(
      new ApiError(409, {
        code: 'knowledgeEvidenceRefreshRequired',
        message: 'The inspected evidence set is no longer available.',
      }),
    );
    const client = new HttpFileManagerClient();

    await expect(client.generateKnowledgeAnswer(answerRequest('answer-e'))).rejects.toMatchObject({
      code: 'knowledgeEvidenceRefreshRequired',
      status: 409,
    });
  });
});

describe('knowledge answer request conformance (task 0207)', () => {
  it('sends the same answer request shape from the HTTP and desktop adapters', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invoked = vi.mocked(invoke);
    invoked.mockReset();
    invoked.mockResolvedValue(answerFixture('answer-parity'));
    generateKnowledgeAnswer.mockResolvedValue({
      status: 200,
      data: answerFixture('answer-parity'),
    });
    const request = answerRequest('answer-parity');

    await new HttpFileManagerClient().generateKnowledgeAnswer(request);
    await new TauriFileManagerClient().generateKnowledgeAnswer(request);

    expect(generateKnowledgeAnswer).toHaveBeenCalledWith(request, undefined);
    expect(invoked).toHaveBeenCalledWith('generate_knowledge_answer', { request });
  });

  it('cancels the same desktop request id when generation is aborted', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invoked = vi.mocked(invoke);
    invoked.mockReset();
    const controller = new AbortController();
    invoked.mockImplementation((command: string) =>
      command === 'generate_knowledge_answer'
        ? new Promise(() => undefined)
        : Promise.resolve(undefined),
    );

    const pending = new TauriFileManagerClient().generateKnowledgeAnswer(
      answerRequest('answer-desktop'),
      controller.signal,
    );
    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoked).toHaveBeenCalledWith('cancel_knowledge_answer', {
      request: { requestId: 'answer-desktop' },
    });
  });

  it('rejects an already aborted desktop generation without invoking the host', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invoked = vi.mocked(invoke);
    invoked.mockReset();
    const controller = new AbortController();
    controller.abort();

    await expect(
      new TauriFileManagerClient().generateKnowledgeAnswer(
        answerRequest('answer-aborted'),
        controller.signal,
      ),
    ).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoked).not.toHaveBeenCalled();
  });
});

describe('MockFileManagerClient knowledge answers (task 0207)', () => {
  it('reports no answer capability until a generation profile exists', async () => {
    const client = new MockFileManagerClient();

    await expect(client.getKnowledgeCapabilities()).resolves.toMatchObject({
      answerGeneration: false,
    });
    const searched = await mockSearch(client);
    expect(searched.capabilities.answerGeneration).toBe(false);

    await configureMockProfile(client);

    await expect(client.getKnowledgeCapabilities()).resolves.toMatchObject({
      answerGeneration: true,
    });
  });

  it('refuses to answer when no generation profile is configured', async () => {
    const client = new MockFileManagerClient();
    const searched = await mockSearch(client);

    await expect(
      client.generateKnowledgeAnswer({
        requestId: 'answer-none',
        workspaceId,
        evidenceFingerprint: searched.evidenceFingerprint,
        profileId: 'missing-profile',
        allowModelKnowledge: false,
      }),
    ).rejects.toMatchObject({ code: 'unavailable' });
  });

  it('answers from the cached evidence set and cites the displayed identities', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    const searched = await mockSearch(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');

    const answer = await client.generateKnowledgeAnswer({
      requestId: 'answer-cached',
      workspaceId,
      evidenceFingerprint: searched.evidenceFingerprint,
      profileId: profile.id,
      allowModelKnowledge: false,
    });

    expect(execute).not.toHaveBeenCalled();
    expect(answer.evidenceFingerprint).toBe(searched.evidenceFingerprint);
    expect(answer.profileName).toBe(profile.name);
    expect(answer.modelKnowledgeAllowed).toBe(false);
    expect(answer.citations.length).toBeGreaterThan(0);
    for (const citation of answer.citations) {
      const displayed = searched.evidence.find((row) => row.recordId === citation.recordId);
      expect(displayed).toBeDefined();
      expect(citation.sourceId).toBe(displayed?.sourceId);
      expect(citation.finalRank).toBe(displayed?.finalRank);
      expect(answer.text).toContain(`[${citation.label}]`);
    }
  });

  it('is deterministic for the same cached evidence set', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    const searched = await mockSearch(client);

    const first = await client.generateKnowledgeAnswer({
      requestId: 'answer-first',
      workspaceId,
      evidenceFingerprint: searched.evidenceFingerprint,
      profileId: profile.id,
      allowModelKnowledge: false,
    });
    const second = await client.generateKnowledgeAnswer({
      requestId: 'answer-second',
      workspaceId,
      evidenceFingerprint: searched.evidenceFingerprint,
      profileId: profile.id,
      allowModelKnowledge: false,
    });

    expect({ ...first, requestId: '' }).toEqual({ ...second, requestId: '' });
  });

  it('labels a model-knowledge answer as permitted rather than silently mixing it in', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    const searched = await mockSearch(client);

    const answer = await client.generateKnowledgeAnswer({
      requestId: 'answer-model',
      workspaceId,
      evidenceFingerprint: searched.evidenceFingerprint,
      profileId: profile.id,
      allowModelKnowledge: true,
    });

    expect(answer.modelKnowledgeAllowed).toBe(true);
  });

  it('requires a fresh search instead of retrieving again for an unknown evidence set', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    await mockSearch(client);
    const execute = vi.spyOn(client, 'executeKnowledgeSearch');

    await expect(
      client.generateKnowledgeAnswer({
        requestId: 'answer-stale',
        workspaceId,
        evidenceFingerprint: 'mock-knowledge-never-searched',
        profileId: profile.id,
        allowModelKnowledge: false,
      }),
    ).rejects.toMatchObject({ code: 'knowledgeEvidenceRefreshRequired' });
    expect(execute).not.toHaveBeenCalled();
  });

  it('cancels a running generation by request id', async () => {
    const client = new MockFileManagerClient({ latencyMs: 20 });
    const profile = await configureMockProfile(client);
    const searched = await mockSearch(client);

    const pending = client.generateKnowledgeAnswer({
      requestId: 'answer-cancelled',
      workspaceId,
      evidenceFingerprint: searched.evidenceFingerprint,
      profileId: profile.id,
      allowModelKnowledge: false,
    });
    const settled = expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    await client.cancelKnowledgeAnswer({ requestId: 'answer-cancelled' });

    await settled;
  });

  it('rejects an already aborted generation before anything else is inspected', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    const searched = await mockSearch(client);
    const controller = new AbortController();
    controller.abort();

    await expect(
      client.generateKnowledgeAnswer(
        {
          requestId: 'answer-preaborted',
          workspaceId,
          evidenceFingerprint: searched.evidenceFingerprint,
          profileId: profile.id,
          allowModelKnowledge: false,
        },
        controller.signal,
      ),
    ).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('bounds the retained evidence sets rather than growing without limit', async () => {
    const client = new MockFileManagerClient();
    const profile = await configureMockProfile(client);
    const first = await client.executeKnowledgeSearch({
      requestId: 'search-first',
      draft: knowledgeDraft({ about: ['retrieval'] }),
      scope: knowledgeScope(),
      mode: 'fullText',
      options: knowledgeOptions(),
    });
    for (let index = 0; index < 8; index += 1) {
      await client.executeKnowledgeSearch({
        requestId: `search-${index}`,
        draft: knowledgeDraft({ about: [`retrieval variant ${index}`] }),
        scope: knowledgeScope(),
        mode: 'fullText',
        options: knowledgeOptions(),
      });
    }

    await expect(
      client.generateKnowledgeAnswer({
        requestId: 'answer-evicted',
        workspaceId,
        evidenceFingerprint: first.evidenceFingerprint,
        profileId: profile.id,
        allowModelKnowledge: false,
      }),
    ).rejects.toMatchObject({ code: 'knowledgeEvidenceRefreshRequired' });
  });
});
