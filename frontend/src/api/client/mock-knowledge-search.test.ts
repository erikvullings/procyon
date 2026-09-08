import { describe, expect, it } from 'vitest';

import type { KnowledgeScope } from '../../models';
import {
  KNOWLEDGE_ARCHIVE_ROOT_ID,
  KNOWLEDGE_HANDBOOK_FOLDER,
  KNOWLEDGE_HANDBOOK_ROOT_ID,
  KNOWLEDGE_ONBOARDING_SOURCE_ID,
  KNOWLEDGE_ONBOARDING_URI,
  KNOWLEDGE_PARSE_CONFORMANCE_CASES,
  KNOWLEDGE_SCOPE_CONFORMANCE_CASES,
  KNOWLEDGE_WORKSPACE_ID,
  knowledgeDraft,
  knowledgeScope,
} from './knowledge-search-fixtures';
import { MockFileManagerClient } from './mock-file-manager-client';
import {
  mockKnowledgeCoverageIsPartial,
  mockKnowledgeScopeIsExact,
  mockKnowledgeUriIsWithin,
} from './mock-knowledge-search';

const workspaceId = KNOWLEDGE_WORKSPACE_ID;

function entireLibrary(): KnowledgeScope {
  return knowledgeScope();
}

describe('MockFileManagerClient knowledge search (task 0206)', () => {
  it('reports full text without embeddings or answer generation', async () => {
    await expect(new MockFileManagerClient().getKnowledgeCapabilities()).resolves.toEqual({
      fullText: true,
      semantic: false,
      answerGeneration: false,
    });
  });

  it('lists indexed roots including an unavailable one', async () => {
    const roots = await new MockFileManagerClient().listKnowledgeRoots({ workspaceId });

    expect(roots.map((root) => root.label)).toEqual(['Handbook', 'Archive']);
    expect(roots.find((root) => root.label === 'Archive')?.available).toBe(false);
  });

  it('interprets explicit DSL and round-trips it to the same canonical text', async () => {
    const client = new MockFileManagerClient();

    const parsed = await client.parseKnowledgeQuery({
      text: 'about: hybrid retrieval need: definition, limitations related: fusion',
    });

    expect(parsed.confidence).toBe('explicit');
    expect(parsed.draft.about).toEqual(['hybrid retrieval']);
    expect(parsed.draft.needs).toEqual(['definition', 'limitations']);
    expect(parsed.draft.related).toEqual(['fusion']);
    expect(parsed.dslCompact).toBe(
      'about: "hybrid retrieval" need: definition, limitations related: fusion',
    );
    const reparsed = await client.parseKnowledgeQuery({ text: parsed.dslMultiline });
    expect(reparsed.draft).toEqual(parsed.draft);
    expect(reparsed.dslCompact).toBe(parsed.dslCompact);
  });

  it('reads plain text as a subject deterministically', async () => {
    const parsed = await new MockFileManagerClient().parseKnowledgeQuery({
      text: 'reciprocal rank fusion',
    });

    expect(parsed.confidence).toBe('deterministic');
    expect(parsed.draft.about).toEqual(['reciprocal rank fusion']);
  });

  it('records an alternative reading instead of discarding it', async () => {
    const parsed = await new MockFileManagerClient().parseKnowledgeQuery({
      text: 'compare hybrid and dense retrieval',
    });

    expect(parsed.confidence).toBe('ambiguous');
    expect(parsed.ambiguities).toHaveLength(1);
    expect(parsed.ambiguities[0]?.alternativeAction).toBe('compare');
  });

  it('diagnoses an unknown field and an unknown need', async () => {
    const parsed = await new MockFileManagerClient().parseKnowledgeQuery({
      text: 'about: retrieval nonsense: x need: whatever',
    });

    expect(parsed.diagnostics.map((diagnostic) => diagnostic.code)).toEqual([
      'unknownField',
      'invalidNeedValue',
    ]);
    expect(parsed.diagnostics[0]?.severity).toBe('error');
  });

  it('keeps answer-only fields out of retrieval but visible in the plan', async () => {
    const client = new MockFileManagerClient();
    const parsed = await client.parseKnowledgeQuery({
      text: 'about: retrieval do: apply to: "write a scheduler" constraint: "no unsafe" format: steps depth: brief',
    });

    expect(parsed.excludedFromRetrieval).toEqual([
      { field: 'do', value: 'apply' },
      { field: 'to', value: 'write a scheduler' },
      { field: 'constraint', value: 'no unsafe' },
      { field: 'format', value: 'steps' },
      { field: 'depth', value: 'brief' },
    ]);

    const plan = await client.planKnowledgeSearch({
      draft: parsed.draft,
      scope: entireLibrary(),
      mode: 'hybrid',
      options: null,
    });

    expect(plan.excludedFromRetrieval).toEqual(parsed.excludedFromRetrieval);
    for (const search of plan.searches) {
      expect(search.text).not.toContain('write a scheduler');
      expect(search.text).not.toContain('no unsafe');
    }
  });

  it('plans the raw subject first, then conservative need expansions', async () => {
    const plan = await new MockFileManagerClient().planKnowledgeSearch({
      draft: {
        about: ['retrieval'],
        needs: ['definition', 'limitations'],
        related: ['fusion'],
        scopes: [],
      },
      scope: entireLibrary(),
      mode: 'hybrid',
      options: null,
    });

    expect(plan.version).toBe('structured-knowledge-planner/1');
    expect(plan.searches.map((search) => search.text)).toEqual([
      'retrieval',
      'retrieval definition',
      'retrieval limitations',
      'fusion',
    ]);
    expect(plan.searches.map((search) => search.priority)).toEqual([
      'primary',
      'secondary',
      'secondary',
      'related',
    ]);
    expect(plan.scopeLabel).toBe('Entire indexed library');
  });

  it('matches the canonical action default needs', async () => {
    const client = new MockFileManagerClient();
    const plans = await Promise.all(
      (
        [
          ['learn', ['retrieval', 'retrieval overview', 'retrieval examples']],
          [
            'evaluate',
            ['retrieval', 'retrieval evidence', 'retrieval arguments', 'retrieval limitations'],
          ],
        ] as const
      ).map(async ([action, searches]) => {
        const plan = await client.planKnowledgeSearch({
          draft: {
            about: ['retrieval'],
            needs: [],
            related: [],
            action,
            scopes: [],
          },
          scope: entireLibrary(),
          mode: 'fullText',
          options: null,
        });
        return [plan.searches.map((search) => search.text), searches] as const;
      }),
    );

    for (const [actual, expected] of plans) expect(actual).toEqual(expected);
  });

  it('retains every reason when two expansions produce the same query', async () => {
    const plan = await new MockFileManagerClient().planKnowledgeSearch({
      draft: { about: ['retrieval'], needs: ['definition'], related: ['retrieval'], scopes: [] },
      scope: entireLibrary(),
      mode: 'hybrid',
      options: null,
    });

    const subjectSearch = plan.searches.find((search) => search.text === 'retrieval');
    expect(subjectSearch?.reasons.map((reason) => reason.kind)).toEqual(['subject', 'relatedTerm']);
  });

  it('executes a full search with no model and explains the full-text fallback', async () => {
    const result = await new MockFileManagerClient().executeKnowledgeSearch({
      requestId: 'request-a',
      draft: { about: ['retrieval'], needs: ['definition'], related: [], scopes: [] },
      scope: entireLibrary(),
      mode: 'hybrid',
      options: null,
    });

    expect(result.capabilities.answerGeneration).toBe(false);
    expect(result.route).toEqual({
      requested: 'hybrid',
      applied: 'fullText',
      fallbackReason: 'queryEmbeddingsUnavailable',
    });
    expect(result.evidence.length).toBeGreaterThan(0);
    expect(result.evidence[0]?.finalRank).toBe(1);
    expect(result.evidence[0]?.rankContributions[0]?.route).toBe('fullText');
    expect(result.evidence.every((row) => row.reasons.length > 0)).toBe(true);
    expect(result.evidence.some((row) => row.stale === true)).toBe(true);
  });

  it('produces identical evidence for identical requests', async () => {
    const client = new MockFileManagerClient();
    const request = {
      draft: { about: ['retrieval'], needs: ['definition' as const], related: [], scopes: [] },
      scope: entireLibrary(),
      mode: 'fullText' as const,
      options: null,
    };

    const first = await client.executeKnowledgeSearch({ ...request, requestId: 'a' });
    const second = await client.executeKnowledgeSearch({ ...request, requestId: 'b' });

    expect(first.evidence.map((row) => row.recordId)).toEqual(
      second.evidence.map((row) => row.recordId),
    );
    expect(first.evidenceFingerprint).toBe(second.evidenceFingerprint);
  });

  it('diversifies results across documents rather than filling one file', async () => {
    const result = await new MockFileManagerClient().executeKnowledgeSearch({
      requestId: 'request-b',
      draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
      scope: entireLibrary(),
      mode: 'fullText',
      options: null,
    });

    const perDocument = new Map<string, number>();
    for (const row of result.evidence)
      perDocument.set(row.documentId, (perDocument.get(row.documentId) ?? 0) + 1);
    expect(Math.max(...perDocument.values())).toBeLessThanOrEqual(3);
    expect(perDocument.size).toBeGreaterThan(1);
  });

  it('refuses a semantic-only search when query embeddings are unavailable', async () => {
    await expect(
      new MockFileManagerClient().executeKnowledgeSearch({
        requestId: 'request-c',
        draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
        scope: entireLibrary(),
        mode: 'semantic',
        options: null,
      }),
    ).rejects.toMatchObject({ code: 'unavailable' });
  });

  it('honours an aborted signal', async () => {
    const client = new MockFileManagerClient({ latencyMs: 20 });
    const controller = new AbortController();

    const pending = expect(
      client.executeKnowledgeSearch(
        {
          requestId: 'request-d',
          draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
          scope: entireLibrary(),
          mode: 'fullText',
          options: null,
        },
        controller.signal,
      ),
    ).rejects.toMatchObject({ name: 'AbortError' });
    controller.abort();

    await pending;
  });

  it('cancels a running search by request id, matching the desktop host', async () => {
    const client = new MockFileManagerClient({ latencyMs: 20 });

    const pending = expect(
      client.executeKnowledgeSearch({
        requestId: 'request-e',
        draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
        scope: entireLibrary(),
        mode: 'fullText',
        options: null,
      }),
    ).rejects.toMatchObject({ name: 'AbortError' });
    await client.cancelKnowledgeSearch({ requestId: 'request-e' });

    await pending;
  });

  it('returns a bounded privacy-safe trace only when asked', async () => {
    const client = new MockFileManagerClient();
    const base = {
      draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
      scope: entireLibrary(),
      mode: 'fullText' as const,
    };

    const without = await client.executeKnowledgeSearch({
      ...base,
      requestId: 'request-f',
      options: null,
    });
    const withTrace = await client.executeKnowledgeSearch({
      ...base,
      requestId: 'request-g',
      options: {
        maximumSearches: 8,
        candidateLimit: 64,
        resultLimit: 20,
        maximumResultsPerFile: 3,
        contextTokenBudget: 8_192,
        adjacentChunkRadius: 1,
        sectionBoundedContext: true,
        includeTrace: true,
      },
    });

    expect(without.trace).toBeNull();
    expect(withTrace.trace?.rankConstant).toBe(60);
    expect(withTrace.trace?.queries[0]?.text).toBe('retrieval');
    expect(JSON.stringify(withTrace.trace)).not.toContain('mock:///');
  });

  it('scopes retrieval to the selected indexed roots', async () => {
    const result = await new MockFileManagerClient().executeKnowledgeSearch({
      requestId: 'request-h',
      draft: { about: ['retrieval'], needs: [], related: [], scopes: [] },
      scope: {
        workspaceId,
        kind: 'enrolledRoots',
        folder: null,
        enrolledRootIds: ['mock-knowledge-root-archive'],
        semanticSourceIds: [],
      },
      mode: 'fullText',
      options: null,
    });

    expect(result.evidence.every((row) => row.title === 'Archived retrieval memo')).toBe(true);
    expect(result.coverage.partial).toBe(true);
  });

  it('resolves a known evidence source and rejects an unknown one', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.resolveKnowledgeSource({
        workspaceId,
        sourceId: 'mock-knowledge-source-onboarding',
      }),
    ).resolves.toMatchObject({ available: true });
    await expect(
      client.resolveKnowledgeSource({ workspaceId, sourceId: 'nope' }),
    ).rejects.toMatchObject({ code: 'notFound' });
  });

  it('reports an unavailable source rather than pretending it opens', async () => {
    await expect(
      new MockFileManagerClient().resolveKnowledgeSource({
        workspaceId,
        sourceId: 'mock-knowledge-source-archive',
      }),
    ).resolves.toMatchObject({ available: false });
  });

  it('rejects a search with no subject', async () => {
    await expect(
      new MockFileManagerClient().planKnowledgeSearch({
        draft: { about: [], needs: ['overview'], related: [], scopes: [] },
        scope: entireLibrary(),
        mode: 'hybrid',
        options: null,
      }),
    ).rejects.toMatchObject({ code: 'invalidRequest' });
  });
});

describe('mock knowledge query language conformance (task 0206)', () => {
  it.each(KNOWLEDGE_PARSE_CONFORMANCE_CASES)('$name', async (parseCase) => {
    const parsed = await new MockFileManagerClient().parseKnowledgeQuery({ text: parseCase.text });

    for (const [field, expected] of Object.entries(parseCase.draft)) {
      expect(parsed.draft[field as keyof typeof parsed.draft], field).toEqual(expected);
    }
    if (parseCase.diagnostics !== undefined) {
      expect(parsed.diagnostics.map((diagnostic) => diagnostic.code)).toEqual(
        parseCase.diagnostics,
      );
    }
  });

  it('round-trips a quoted subject through its own canonical DSL', async () => {
    const client = new MockFileManagerClient();

    const parsed = await client.parseKnowledgeQuery({ text: 'about: "ACME, Inc." need: overview' });
    const reparsed = await client.parseKnowledgeQuery({ text: parsed.dslMultiline });

    expect(parsed.draft.about).toEqual(['ACME, Inc.']);
    expect(reparsed.draft).toEqual(parsed.draft);
  });
});

describe('mock knowledge scope conformance (task 0206)', () => {
  it.each(KNOWLEDGE_SCOPE_CONFORMANCE_CASES)('$name', async (scopeCase) => {
    const client = new MockFileManagerClient();
    const request = {
      requestId: `scope-${scopeCase.name}`,
      draft: scopeCase.draft ?? knowledgeDraft(),
      scope: scopeCase.scope,
      mode: 'fullText' as const,
      options: null,
    };

    if (scopeCase.expect.kind === 'refused') {
      await expect(client.executeKnowledgeSearch(request)).rejects.toMatchObject({
        code: scopeCase.expect.code,
      });
      return;
    }
    const result = await client.executeKnowledgeSearch(request);
    expect(result.plan.authorizedSources).toBe(scopeCase.expect.titles.length);
    expect(result.coverage.eligible).toBe(scopeCase.expect.titles.length);
    for (const row of result.evidence) expect(scopeCase.expect.titles).toContain(row.title);
  });

  it('treats a folder as containing itself but not a same-prefixed sibling', () => {
    expect(mockKnowledgeUriIsWithin('mock:///a/b', 'mock:///a/b')).toBe(true);
    expect(mockKnowledgeUriIsWithin('mock:///a/b/c.md', 'mock:///a/b/')).toBe(true);
    expect(mockKnowledgeUriIsWithin('mock:///a/b-old/c.md', 'mock:///a/b')).toBe(false);
  });
});

describe('mock knowledge coverage honesty (task 0206)', () => {
  it('reports partial coverage from measured gaps, not from a smaller scope', async () => {
    const client = new MockFileManagerClient();

    const wholeLibrary = await client.executeKnowledgeSearch({
      requestId: 'coverage-a',
      draft: knowledgeDraft(),
      scope: knowledgeScope(),
      mode: 'fullText',
      options: null,
    });
    const oneFreshSource = await client.executeKnowledgeSearch({
      requestId: 'coverage-b',
      draft: knowledgeDraft({ about: ['onboarding'] }),
      scope: knowledgeScope({
        kind: 'currentFolder',
        folder: { providerId: 'file', uri: KNOWLEDGE_ONBOARDING_URI },
      }),
      mode: 'fullText',
      options: null,
    });

    // The whole library contains an unavailable source, so it is partial.
    expect(wholeLibrary.coverage.unavailable).toBe(1);
    expect(wholeLibrary.coverage.partial).toBe(true);
    // A single fresh, available source is a *smaller* scope, not a partial one.
    expect(oneFreshSource.coverage.eligible).toBe(1);
    expect(oneFreshSource.coverage.unavailable).toBe(0);
    expect(oneFreshSource.coverage.staleEvidence).toBe(0);
    expect(oneFreshSource.coverage.scopeIsExact).toBe(true);
    expect(oneFreshSource.coverage.partial).toBe(false);
  });

  it('reports a listable semantic-result scope as exact, like the backend does', async () => {
    const result = await new MockFileManagerClient().executeKnowledgeSearch({
      requestId: 'coverage-c',
      draft: knowledgeDraft({ about: ['onboarding'] }),
      scope: knowledgeScope({
        kind: 'semanticResults',
        semanticSourceIds: [KNOWLEDGE_ONBOARDING_SOURCE_ID],
      }),
      mode: 'fullText',
      options: null,
    });

    // The scope is not expressible as worker filters, but it is still ranked
    // exactly because its authorized sources can be listed explicitly.
    expect(result.coverage.scopeIsExact).toBe(true);
    expect(result.coverage.partial).toBe(false);
  });

  it('treats library and root scopes as filter-expressible and exact', () => {
    expect(mockKnowledgeScopeIsExact(knowledgeScope())).toBe(true);
    expect(
      mockKnowledgeScopeIsExact(
        knowledgeScope({ kind: 'enrolledRoots', enrolledRootIds: [KNOWLEDGE_ARCHIVE_ROOT_ID] }),
      ),
    ).toBe(true);
    expect(
      mockKnowledgeScopeIsExact(
        knowledgeScope({ kind: 'currentFolder', folder: KNOWLEDGE_HANDBOOK_FOLDER }),
      ),
    ).toBe(true);
  });

  it('mirrors the backend partial rule exactly', () => {
    const base = {
      eligible: 4,
      indexed: 4,
      unavailable: 0,
      staleEvidence: 0,
      scopeIsExact: true,
    };

    expect(mockKnowledgeCoverageIsPartial(base)).toBe(false);
    expect(mockKnowledgeCoverageIsPartial({ ...base, scopeIsExact: false })).toBe(true);
    expect(mockKnowledgeCoverageIsPartial({ ...base, unavailable: 1 })).toBe(true);
    expect(mockKnowledgeCoverageIsPartial({ ...base, staleEvidence: 1 })).toBe(true);
    expect(mockKnowledgeCoverageIsPartial({ ...base, indexed: 3 })).toBe(true);
    // Unknown publication state is reported, never folded into "partial".
    expect(mockKnowledgeCoverageIsPartial({ ...base, indexed: null })).toBe(false);
  });
});

describe('mock knowledge cancellation semantics (task 0206)', () => {
  it('rejects an already-aborted search before any other refusal', async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      new MockFileManagerClient().executeKnowledgeSearch(
        {
          requestId: 'aborted-a',
          // A route the mock cannot run: cancellation still wins.
          draft: knowledgeDraft(),
          scope: knowledgeScope(),
          mode: 'semantic',
          options: null,
        },
        controller.signal,
      ),
    ).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('rejects with AbortError when the request id is cancelled mid-flight', async () => {
    const client = new MockFileManagerClient({ latencyMs: 20 });

    const pending = expect(
      client.executeKnowledgeSearch({
        requestId: 'aborted-b',
        draft: knowledgeDraft(),
        scope: knowledgeScope(),
        mode: 'fullText',
        options: null,
      }),
    ).rejects.toMatchObject({ name: 'AbortError' });
    await client.cancelKnowledgeSearch({ requestId: 'aborted-b' });

    await pending;
  });
});

describe('mock knowledge DSL scope promotion (task 0206)', () => {
  it('scopes a DSL root selector the same way an explicit root selection does', async () => {
    const client = new MockFileManagerClient();

    const promoted = await client.planKnowledgeSearch({
      draft: knowledgeDraft({ scopes: [{ kind: 'root', id: KNOWLEDGE_HANDBOOK_ROOT_ID }] }),
      scope: knowledgeScope(),
      mode: 'fullText',
      options: null,
    });
    const explicit = await client.planKnowledgeSearch({
      draft: knowledgeDraft(),
      scope: knowledgeScope({
        kind: 'enrolledRoots',
        enrolledRootIds: [KNOWLEDGE_HANDBOOK_ROOT_ID],
      }),
      mode: 'fullText',
      options: null,
    });

    expect(promoted.scope.kind).toBe('enrolledRoots');
    expect(promoted.scope.enrolledRootIds).toEqual([KNOWLEDGE_HANDBOOK_ROOT_ID]);
    expect(promoted.authorizedSources).toBe(explicit.authorizedSources);
    expect(promoted.scopeLabel).toBe(explicit.scopeLabel);
  });

  it('keeps a folder scope inside its own root', async () => {
    const result = await new MockFileManagerClient().executeKnowledgeSearch({
      requestId: 'folder-a',
      draft: knowledgeDraft(),
      scope: knowledgeScope({ kind: 'currentFolder', folder: KNOWLEDGE_HANDBOOK_FOLDER }),
      mode: 'fullText',
      options: null,
    });

    expect(result.evidence.every((row) => row.title !== 'Archived retrieval memo')).toBe(true);
    expect(result.coverage.eligible).toBe(2);
  });

  it('refuses a DSL root selector the host never reported', async () => {
    await expect(
      new MockFileManagerClient().planKnowledgeSearch({
        draft: knowledgeDraft({ scopes: [{ kind: 'root', id: 'mock-knowledge-root-nope' }] }),
        scope: knowledgeScope(),
        mode: 'fullText',
        options: null,
      }),
    ).rejects.toMatchObject({ code: 'invalidRequest' });
  });

  it('lets an explicit root selection win over a DSL selector', async () => {
    const plan = await new MockFileManagerClient().planKnowledgeSearch({
      draft: knowledgeDraft({ scopes: [{ kind: 'root', id: KNOWLEDGE_HANDBOOK_ROOT_ID }] }),
      scope: knowledgeScope({
        kind: 'enrolledRoots',
        enrolledRootIds: [KNOWLEDGE_ARCHIVE_ROOT_ID],
      }),
      mode: 'fullText',
      options: null,
    });

    expect(plan.scope.enrolledRootIds).toEqual([KNOWLEDGE_ARCHIVE_ROOT_ID]);
  });
});
