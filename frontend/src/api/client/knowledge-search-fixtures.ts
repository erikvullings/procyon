import type {
  KnowledgeQueryDraft,
  KnowledgeScope,
  KnowledgeScopeSelector,
  KnowledgeSearchOptions,
  Location,
} from '../../models';
import { defaultKnowledgeSearchOptions } from '../../models';

/**
 * Shared knowledge-search fixtures and conformance cases (task 0206).
 *
 * Every host adapter has to agree on the same query language, the same scope
 * resolution and the same honest coverage reporting, so the expectations live
 * here once instead of being restated - and quietly drifting - in the mock
 * tests, the adapter parity tests and the dialog tests.
 */

export const KNOWLEDGE_WORKSPACE_ID = '22222222-2222-4222-8222-222222222222';

export const KNOWLEDGE_HANDBOOK_ROOT_ID = 'mock-knowledge-root-handbook';
export const KNOWLEDGE_ARCHIVE_ROOT_ID = 'mock-knowledge-root-archive';

export const KNOWLEDGE_HANDBOOK_FOLDER: Location = {
  providerId: 'file',
  uri: 'mock:///Documents/handbook',
};
/** A sibling whose URI merely starts with the handbook folder's characters. */
export const KNOWLEDGE_HANDBOOK_SIBLING_FOLDER: Location = {
  providerId: 'file',
  uri: 'mock:///Documents/handbook-archive',
};
export const KNOWLEDGE_ONBOARDING_URI = 'mock:///Documents/handbook/onboarding.md';
export const KNOWLEDGE_RETRIEVAL_URI = 'mock:///Documents/handbook/retrieval.md';

export const KNOWLEDGE_ONBOARDING_SOURCE_ID = 'mock-knowledge-source-onboarding';
export const KNOWLEDGE_RETRIEVAL_SOURCE_ID = 'mock-knowledge-source-retrieval';
export const KNOWLEDGE_ARCHIVE_SOURCE_ID = 'mock-knowledge-source-archive';

/** A knowledge scope with every field explicit, so nothing is inferred. */
export function knowledgeScope(overrides: Partial<KnowledgeScope> = {}): KnowledgeScope {
  return {
    workspaceId: KNOWLEDGE_WORKSPACE_ID,
    kind: 'entireLibrary',
    folder: null,
    enrolledRootIds: [],
    semanticSourceIds: [],
    ...overrides,
  };
}

/** A canonical draft with every field explicit. */
export function knowledgeDraft(overrides: Partial<KnowledgeQueryDraft> = {}): KnowledgeQueryDraft {
  return {
    about: ['retrieval'],
    needs: [],
    related: [],
    scopes: [],
    action: null,
    context: null,
    constraints: [],
    format: null,
    depth: null,
    ...overrides,
  };
}

/** Bounded options with an explicit trace request. */
export function knowledgeOptions(includeTrace = false): KnowledgeSearchOptions {
  return { ...defaultKnowledgeSearchOptions(), includeTrace };
}

/** One documented parsing behaviour every conformant parser must reproduce. */
export interface KnowledgeParseCase {
  readonly name: string;
  readonly text: string;
  /** Only the draft fields this case is about; the rest are not asserted. */
  readonly draft: Partial<KnowledgeQueryDraft>;
  /** Diagnostic codes expected in order, when the case is about diagnostics. */
  readonly diagnostics?: readonly string[];
}

/**
 * Query-language conformance, mirroring `fm-application::knowledge_dsl`:
 * quote-aware field scanning, the documented field and value aliases, and
 * scope selectors that survive parsing instead of being read as fields.
 */
export const KNOWLEDGE_PARSE_CONFORMANCE_CASES: readonly KnowledgeParseCase[] = [
  {
    name: 'keeps a quoted subject containing a comma as one value',
    text: 'about: "ACME, Inc.", fusion',
    draft: { about: ['ACME, Inc.', 'fusion'] },
    diagnostics: [],
  },
  {
    name: 'keeps a colon inside a quoted value out of field scanning',
    text: 'about: "ACME, Inc.: east" need: definition',
    draft: { about: ['ACME, Inc.: east'], needs: ['definition'] },
    diagnostics: [],
  },
  {
    name: 'reads a scope selector as a value, not as an unknown field',
    text: `about: retrieval scope: root:${KNOWLEDGE_HANDBOOK_ROOT_ID}`,
    draft: {
      about: ['retrieval'],
      scopes: [{ kind: 'root', id: KNOWLEDGE_HANDBOOK_ROOT_ID }],
    },
    diagnostics: [],
  },
  {
    name: 'accepts the documented field aliases',
    text: 'subject: retrieval needs: define see-also: fusion',
    draft: { about: ['retrieval'], needs: ['definition'], related: ['fusion'] },
    diagnostics: [],
  },
  {
    name: 'accepts hyphen and underscore spellings of one alias',
    text: 'topic: retrieval information: how-to detail-level: short',
    draft: { about: ['retrieval'], needs: ['procedure'], depth: 'brief' },
    diagnostics: [],
  },
  {
    name: 'accepts the documented value aliases',
    text: 'about: retrieval need: risks, sources do: implement output: list',
    draft: {
      about: ['retrieval'],
      needs: ['limitations', 'references'],
      action: 'apply',
      format: 'bullets',
    },
    diagnostics: [],
  },
  {
    name: 'accepts the whole-library scope selector',
    text: 'about: retrieval scope: library',
    draft: { about: ['retrieval'], scopes: [{ kind: 'wholeLibrary', id: null }] },
    diagnostics: [],
  },
  {
    name: 'reports an unknown field with an actionable diagnostic',
    text: 'about: retrieval nonsense: x',
    draft: { about: ['retrieval'] },
    diagnostics: ['unknownField'],
  },
  {
    name: 'reports an unterminated quote',
    text: 'about: "retrieval',
    draft: {},
    diagnostics: ['unterminatedQuote'],
  },
];

/** One scope-resolution behaviour every conformant host must reproduce. */
export interface KnowledgeScopeCase {
  readonly name: string;
  readonly scope: KnowledgeScope;
  readonly draft?: KnowledgeQueryDraft;
  /** Document titles expected in the resolved scope, or the refusal code. */
  readonly expect:
    | { readonly kind: 'documents'; readonly titles: readonly string[] }
    | { readonly kind: 'refused'; readonly code: 'invalidRequest' | 'notFound' };
}

/**
 * Scope conformance, mirroring `semantic_library::resolve_scope_sources` and
 * `knowledge_service::scope_selection`.
 */
export const KNOWLEDGE_SCOPE_CONFORMANCE_CASES: readonly KnowledgeScopeCase[] = [
  {
    name: 'the entire library covers every authorized source',
    scope: knowledgeScope(),
    expect: {
      kind: 'documents',
      titles: ['Onboarding handbook', 'Retrieval design notes', 'Archived retrieval memo'],
    },
  },
  {
    name: 'a folder scope covers exactly the sources beneath it',
    scope: knowledgeScope({ kind: 'currentFolder', folder: KNOWLEDGE_HANDBOOK_FOLDER }),
    expect: { kind: 'documents', titles: ['Onboarding handbook', 'Retrieval design notes'] },
  },
  {
    name: 'a folder scope stops at the path boundary',
    scope: knowledgeScope({ kind: 'currentFolder', folder: KNOWLEDGE_HANDBOOK_SIBLING_FOLDER }),
    expect: { kind: 'refused', code: 'notFound' },
  },
  {
    name: 'a folder scope never crosses providers',
    scope: knowledgeScope({
      kind: 'currentFolder',
      folder: { providerId: 'sftp', uri: KNOWLEDGE_HANDBOOK_FOLDER.uri },
    }),
    expect: { kind: 'refused', code: 'notFound' },
  },
  {
    name: 'a folder scope without a folder is refused',
    scope: knowledgeScope({ kind: 'currentFolder' }),
    expect: { kind: 'refused', code: 'invalidRequest' },
  },
  {
    name: 'an indexed-root scope covers exactly that root',
    scope: knowledgeScope({ kind: 'enrolledRoots', enrolledRootIds: [KNOWLEDGE_ARCHIVE_ROOT_ID] }),
    expect: { kind: 'documents', titles: ['Archived retrieval memo'] },
  },
  {
    name: 'an empty indexed-root scope is refused',
    scope: knowledgeScope({ kind: 'enrolledRoots' }),
    expect: { kind: 'refused', code: 'invalidRequest' },
  },
  {
    name: 'an unknown indexed root is refused',
    scope: knowledgeScope({ kind: 'enrolledRoots', enrolledRootIds: ['mock-knowledge-root-nope'] }),
    expect: { kind: 'refused', code: 'invalidRequest' },
  },
  {
    name: 'a semantic result scope covers exactly those sources',
    scope: knowledgeScope({
      kind: 'semanticResults',
      semanticSourceIds: [KNOWLEDGE_ONBOARDING_SOURCE_ID],
    }),
    expect: { kind: 'documents', titles: ['Onboarding handbook'] },
  },
  {
    name: 'an empty semantic result scope is refused',
    scope: knowledgeScope({ kind: 'semanticResults' }),
    expect: { kind: 'refused', code: 'invalidRequest' },
  },
  {
    name: 'a DSL root selector is promoted when the composer picked no root',
    scope: knowledgeScope(),
    draft: knowledgeDraft({
      scopes: [{ kind: 'root', id: KNOWLEDGE_ARCHIVE_ROOT_ID } as KnowledgeScopeSelector],
    }),
    expect: { kind: 'documents', titles: ['Archived retrieval memo'] },
  },
];
