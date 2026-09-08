import type {
  KnowledgeCapabilities,
  KnowledgeDiagnostic,
  KnowledgeEvidence,
  KnowledgeExcludedField,
  KnowledgeNeed,
  KnowledgeParseAmbiguity,
  KnowledgeParseConfidence,
  KnowledgePlannedSearch,
  KnowledgeQueryDraft,
  KnowledgeQueryInterpretation,
  KnowledgeRankContribution,
  KnowledgeRetrievalMode,
  KnowledgeRoot,
  KnowledgeRouteOutcome,
  KnowledgeScope,
  KnowledgeScopeSelector,
  KnowledgeSearchOptions,
  KnowledgeSearchPlan,
  KnowledgeSearchReason,
  KnowledgeSearchResult,
  KnowledgeSourceLocation,
  KnowledgeTracedQuery,
} from '../../models';

/**
 * Deterministic, LLM-free knowledge search used by {@link
 * MockFileManagerClient} (task 0206).
 *
 * This mirrors the backend contract closely enough to drive the real UI: the
 * same canonical planner shape, the same DSL round-trip, the same
 * reciprocal-rank fusion over per-query ranks, the same per-document
 * diversification, and the same explicit route fallback. Nothing here contacts
 * a model: full text is available, query embeddings are not, and answer
 * generation is off, which is exactly the configuration the search-only UX
 * must remain fully usable in.
 */

/** Planner identity reported by the mock, mirroring the Rust planner's shape. */
export const MOCK_KNOWLEDGE_PLANNER_VERSION = 'structured-knowledge-planner/1';

/** Rank constant used by the mock's reciprocal-rank fusion. */
const RANK_CONSTANT = 60;

const MAX_SUBJECTS = 8;
const MAX_RELATED_TERMS = 16;

/** Canonical need order, matching the backend enum declaration order. */
export const KNOWLEDGE_NEEDS: readonly KnowledgeNeed[] = [
  'overview',
  'definition',
  'procedure',
  'examples',
  'evidence',
  'arguments',
  'comparison',
  'limitations',
  'references',
];

/** Flattens a canonical-value/alias table into a normalized lookup. */
function buildAliases<T extends string>(
  table: Readonly<Record<T, readonly string[]>>,
): Readonly<Record<string, T>> {
  const lookup: Record<string, T> = {};
  for (const [canonical, aliases] of Object.entries(table) as [T, readonly string[]][]) {
    lookup[normalizeKey(canonical)] = canonical;
    for (const alias of aliases) lookup[normalizeKey(alias)] = canonical;
  }
  return lookup;
}

/** Documented need aliases, mirroring `parse_need` in `fm-application::knowledge_dsl`. */
const NEED_ALIASES: Readonly<Record<string, KnowledgeNeed>> = buildAliases({
  overview: [],
  definition: ['define', 'definitions'],
  procedure: ['howTo', 'how-to', 'steps', 'procedures'],
  examples: ['sample', 'example', 'samples'],
  evidence: ['support'],
  arguments: ['prosCons', 'pros-cons', 'argument'],
  comparison: ['compare', 'comparisons'],
  limitations: ['risks', 'limitation', 'risk'],
  references: ['sources', 'reference', 'source'],
});

const ACTIONS = ['explain', 'learn', 'apply', 'evaluate', 'compare', 'cite'] as const;
const FORMATS = ['narrative', 'bullets', 'steps', 'table'] as const;
const DEPTHS = ['brief', 'standard', 'detailed'] as const;

const ACTION_ALIASES: Readonly<Record<string, (typeof ACTIONS)[number]>> = buildAliases({
  explain: [],
  learn: [],
  apply: ['implement', 'howTo', 'use'],
  evaluate: ['analyse', 'analyze'],
  compare: [],
  cite: ['source'],
});

const FORMAT_ALIASES: Readonly<Record<string, (typeof FORMATS)[number]>> = buildAliases({
  narrative: ['prose'],
  bullets: ['list', 'bulleted'],
  steps: ['ordered', 'numbered'],
  table: ['tabular', 'grid'],
});

const DEPTH_ALIASES: Readonly<Record<string, (typeof DEPTHS)[number]>> = buildAliases({
  brief: ['short'],
  standard: ['normal'],
  detailed: ['deep'],
});

const DEFAULT_ACTION_NEEDS: Readonly<Record<string, readonly KnowledgeNeed[]>> = {
  explain: ['overview', 'definition'],
  learn: ['overview', 'examples'],
  apply: ['procedure', 'examples'],
  evaluate: ['evidence', 'arguments', 'limitations'],
  compare: ['comparison', 'evidence'],
  cite: ['references', 'evidence'],
};

interface MockKnowledgeChunk {
  readonly recordId: string;
  readonly sectionPath: readonly string[];
  readonly content: string;
  readonly sourcePosition: number;
  readonly page: number;
}

interface MockKnowledgeDocument {
  readonly documentId: string;
  readonly sourceId: string;
  readonly rootId: string;
  readonly title: string;
  /** Provider owning the source, so folder scopes cannot cross providers. */
  readonly providerId: string;
  readonly uri: string;
  readonly mediaType: string;
  readonly modifiedAtMs: number;
  readonly stale: boolean | null;
  readonly unavailable: boolean;
  readonly chunks: readonly MockKnowledgeChunk[];
}

/** Two indexed roots the mock always exposes, so scope selection is exercisable. */
const MOCK_KNOWLEDGE_ROOTS: readonly KnowledgeRoot[] = [
  {
    rootId: 'mock-knowledge-root-handbook',
    label: 'Handbook',
    location: { providerId: 'file', uri: 'mock:///Documents/handbook' },
    recursive: true,
    indexedGeneration: 4,
    available: true,
  },
  {
    rootId: 'mock-knowledge-root-archive',
    label: 'Archive',
    location: { providerId: 'file', uri: 'mock:///Documents/archive' },
    recursive: false,
    indexedGeneration: 2,
    available: false,
  },
];

const MOCK_KNOWLEDGE_CORPUS: readonly MockKnowledgeDocument[] = [
  {
    documentId: 'mock-knowledge-document-onboarding',
    sourceId: 'mock-knowledge-source-onboarding',
    rootId: 'mock-knowledge-root-handbook',
    title: 'Onboarding handbook',
    providerId: 'file',
    uri: 'mock:///Documents/handbook/onboarding.md',
    mediaType: 'text/markdown',
    modifiedAtMs: Date.UTC(2026, 0, 12),
    stale: false,
    unavailable: false,
    chunks: [
      {
        recordId: 'mock-knowledge-record-onboarding-1',
        sectionPath: ['Onboarding', 'Overview'],
        content:
          'Onboarding overview: this handbook explains how a new maintainer joins the ' +
          'project, which retrieval systems exist, and what the definition of done is.',
        sourcePosition: 0,
        page: 1,
      },
      {
        recordId: 'mock-knowledge-record-onboarding-2',
        sectionPath: ['Onboarding', 'Procedure'],
        content:
          'Procedure: run the indexing steps in order, then verify retrieval with a ' +
          'full-text query before enabling any optional semantic route.',
        sourcePosition: 1,
        page: 2,
      },
      {
        recordId: 'mock-knowledge-record-onboarding-3',
        sectionPath: ['Onboarding', 'Examples'],
        content:
          'Examples: a search for "retrieval" returns ranked evidence with reasons and ' +
          'structural provenance for every row.',
        sourcePosition: 2,
        page: 2,
      },
    ],
  },
  {
    documentId: 'mock-knowledge-document-retrieval',
    sourceId: 'mock-knowledge-source-retrieval',
    rootId: 'mock-knowledge-root-handbook',
    title: 'Retrieval design notes',
    providerId: 'file',
    uri: 'mock:///Documents/handbook/retrieval.md',
    mediaType: 'text/markdown',
    modifiedAtMs: Date.UTC(2026, 1, 3),
    stale: true,
    unavailable: false,
    chunks: [
      {
        recordId: 'mock-knowledge-record-retrieval-1',
        sectionPath: ['Retrieval', 'Definition'],
        content:
          'Definition: hybrid retrieval fuses a full-text ranking and a dense ranking ' +
          'using reciprocal rank fusion, never by adding raw scores.',
        sourcePosition: 0,
        page: 1,
      },
      {
        recordId: 'mock-knowledge-record-retrieval-2',
        sectionPath: ['Retrieval', 'Limitations'],
        content:
          'Limitations: without query embeddings the semantic route is unavailable, so a ' +
          'hybrid request falls back to full-text retrieval and says so.',
        sourcePosition: 1,
        page: 2,
      },
    ],
  },
  {
    documentId: 'mock-knowledge-document-archive',
    sourceId: 'mock-knowledge-source-archive',
    rootId: 'mock-knowledge-root-archive',
    title: 'Archived retrieval memo',
    providerId: 'file',
    uri: 'mock:///Documents/archive/memo.txt',
    mediaType: 'text/plain',
    modifiedAtMs: Date.UTC(2025, 5, 20),
    stale: null,
    unavailable: true,
    chunks: [
      {
        recordId: 'mock-knowledge-record-archive-1',
        sectionPath: ['Memo'],
        content:
          'Evidence: the archived memo records an earlier retrieval experiment and its ' +
          'references, kept for comparison only.',
        sourcePosition: 0,
        page: 1,
      },
    ],
  },
];

/** Knowledge capabilities reported by the mock: full text only, no LLM. */
export function mockKnowledgeCapabilities(): KnowledgeCapabilities {
  return { fullText: true, semantic: false, answerGeneration: false };
}

/** The indexed roots the mock exposes for scope selection. */
export function mockKnowledgeRoots(): KnowledgeRoot[] {
  return MOCK_KNOWLEDGE_ROOTS.map((root) => structuredClone(root));
}

// --- Parsing --------------------------------------------------------------

const FIELD_NAMES = [
  'about',
  'need',
  'related',
  'scope',
  'do',
  'to',
  'constraint',
  'format',
  'depth',
] as const;

type FieldName = (typeof FIELD_NAMES)[number];

/**
 * Documented field aliases, mirroring `KnowledgeDslField::aliases` in
 * `fm-application::knowledge_dsl`. Keys are compared after {@link normalizeKey},
 * so `see-also`, `seeAlso` and `see_also` are the same alias.
 */
const FIELD_ALIASES: Readonly<Record<FieldName, readonly string[]>> = {
  about: ['about', 'subject', 'topic', 'question', 'ask'],
  need: ['need', 'needs', 'info', 'information'],
  related: ['related', 'see-also', 'seealso', 'also'],
  scope: ['scope', 'scopes', 'library'],
  do: ['do', 'action', 'goal'],
  to: ['to', 'context', 'for'],
  constraint: ['constraint', 'constraints', 'must'],
  format: ['format', 'output', 'as'],
  depth: ['depth', 'detail', 'detail-level'],
};

/** Lowercases and strips `-`/`_`, matching the backend's `normalize_key`. */
function normalizeKey(value: string): string {
  return value.replace(/[-_]/gu, '').toLowerCase();
}

/** Resolves a raw DSL key to its canonical field, honouring documented aliases. */
function fieldFromKey(rawKey: string): FieldName | undefined {
  const normalized = normalizeKey(rawKey);
  return FIELD_NAMES.find((field) =>
    FIELD_ALIASES[field].some((alias) => normalizeKey(alias) === normalized),
  );
}

interface FieldOccurrence {
  readonly field: FieldName;
  readonly values: readonly string[];
  readonly start: number;
  readonly end: number;
}

function splitValues(raw: string): string[] {
  const values: string[] = [];
  let current = '';
  let quoted = false;
  for (let index = 0; index < raw.length; index += 1) {
    const character = raw[index];
    if (character === '\\' && quoted && index + 1 < raw.length) {
      current += raw[index + 1] ?? '';
      index += 1;
      continue;
    }
    if (character === '"') {
      quoted = !quoted;
      continue;
    }
    if (character === ',' && !quoted) {
      values.push(current.trim());
      current = '';
      continue;
    }
    current += character;
  }
  values.push(current.trim());
  return values.filter((value) => value.length > 0);
}

function hasUnterminatedQuote(raw: string): boolean {
  let quoted = false;
  for (let index = 0; index < raw.length; index += 1) {
    if (raw[index] === '\\' && quoted) {
      index += 1;
      continue;
    }
    if (raw[index] === '"') quoted = !quoted;
  }
  return quoted;
}

function fieldOccurrences(text: string): {
  readonly occurrences: readonly FieldOccurrence[];
  readonly unknownFields: readonly { readonly name: string; readonly start: number }[];
  readonly leading: string;
} {
  const matches = findFieldBoundaries(text);
  const occurrences: FieldOccurrence[] = [];
  const unknownFields: { name: string; start: number }[] = [];
  for (const [index, entry] of matches.entries()) {
    const next = matches[index + 1];
    const end = next === undefined ? text.length : next.keyStart;
    const raw = text.slice(entry.valueStart, end).replace(/[;\s]+$/u, '');
    const field = fieldFromKey(entry.name);
    if (field === undefined) {
      unknownFields.push({ name: entry.name, start: entry.keyStart });
      continue;
    }
    occurrences.push({
      field,
      values: splitValues(raw),
      start: entry.valueStart,
      end,
    });
  }
  const leading = matches[0] === undefined ? text : text.slice(0, matches[0].keyStart);
  return { occurrences, unknownFields, leading: leading.trim() };
}

/**
 * Finds `key:` assignments outside quoted spans, mirroring
 * `find_field_boundaries` in `fm-application::knowledge_dsl`.
 *
 * A colon only opens a field when the key is at a hard boundary (input start or
 * after `;`), or when what follows the colon is whitespace, a quote or the end
 * of the input, or when the key itself is a documented field alias. That is
 * what keeps `scope: root:handbook` one scope value instead of an unknown
 * `root` field, and what keeps a colon inside `about: "ACME, Inc.: east"` part
 * of the quoted subject.
 */
function findFieldBoundaries(
  text: string,
): readonly { readonly name: string; readonly valueStart: number; readonly keyStart: number }[] {
  const boundaries: { name: string; valueStart: number; keyStart: number }[] = [];
  let quoted = false;
  let index = 0;
  while (index < text.length) {
    const character = text[index] ?? '';
    if (quoted && character === '\\') {
      index += 2;
      continue;
    }
    if (character === '"') {
      quoted = !quoted;
      index += 1;
      continue;
    }
    if (quoted) {
      index += 1;
      continue;
    }
    const previous = index === 0 ? undefined : text[index - 1];
    const hardBoundary = index === 0 || previous === ';';
    const atBoundary = hardBoundary || /\s/u.test(previous ?? '');
    if (atBoundary && /[A-Za-z]/u.test(character)) {
      let end = index;
      while (end < text.length && /[A-Za-z0-9_-]/u.test(text[end] ?? '')) end += 1;
      if (end - index >= 2 && text[end] === ':') {
        const after = end + 1;
        const following = text[after];
        const endsField =
          following === undefined ||
          /\s/u.test(following) ||
          following === '"' ||
          hardBoundary ||
          fieldFromKey(text.slice(index, end)) !== undefined;
        if (endsField) {
          boundaries.push({ name: text.slice(index, end), valueStart: after, keyStart: index });
          index = after;
          continue;
        }
      }
    }
    index += 1;
  }
  return boundaries;
}

function parseScopeSelector(value: string): KnowledgeScopeSelector | undefined {
  const normalized = normalizeKey(value);
  if (normalized === 'library' || normalized === 'wholelibrary') {
    return { kind: 'wholeLibrary', id: null };
  }
  const separator = value.indexOf(':');
  if (separator < 0) return undefined;
  const prefix = normalizeKey(value.slice(0, separator));
  const id = value.slice(separator + 1).trim();
  if (id.length === 0) return undefined;
  if (prefix === 'root') return { kind: 'root', id };
  if (prefix === 'workspace') return { kind: 'workspace', id };
  return undefined;
}

function scopeSelectorText(selector: KnowledgeScopeSelector): string {
  switch (selector.kind) {
    case 'wholeLibrary':
      return 'library';
    case 'root':
      return `root:${selector.id ?? ''}`;
    case 'workspace':
      return `workspace:${selector.id ?? ''}`;
  }
}

function quoteIfNeeded(value: string): string {
  return /[\s,:;"]/u.test(value) ? `"${value.replace(/(["\\])/gu, '\\$1')}"` : value;
}

function fieldLines(draft: KnowledgeQueryDraft): string[] {
  const lines: string[] = [];
  const join = (values: readonly string[]): string => values.map(quoteIfNeeded).join(', ');
  if ((draft.about?.length ?? 0) > 0) lines.push(`about: ${join(draft.about ?? [])}`);
  if ((draft.needs?.length ?? 0) > 0) lines.push(`need: ${join(draft.needs ?? [])}`);
  if ((draft.related?.length ?? 0) > 0) lines.push(`related: ${join(draft.related ?? [])}`);
  if ((draft.scopes?.length ?? 0) > 0)
    lines.push(`scope: ${join((draft.scopes ?? []).map(scopeSelectorText))}`);
  if (draft.action != null) lines.push(`do: ${quoteIfNeeded(draft.action)}`);
  if (draft.context != null && draft.context.length > 0)
    lines.push(`to: ${quoteIfNeeded(draft.context)}`);
  if ((draft.constraints?.length ?? 0) > 0)
    lines.push(`constraint: ${join(draft.constraints ?? [])}`);
  if (draft.format != null) lines.push(`format: ${quoteIfNeeded(draft.format)}`);
  if (draft.depth != null) lines.push(`depth: ${quoteIfNeeded(draft.depth)}`);
  return lines;
}

/** Canonical compact DSL for a draft. */
export function formatKnowledgeDraftCompact(draft: KnowledgeQueryDraft): string {
  return fieldLines(draft).join(' ');
}

/** Canonical multiline DSL for a draft. */
export function formatKnowledgeDraftMultiline(draft: KnowledgeQueryDraft): string {
  return fieldLines(draft).join('\n');
}

/** Answer-only fields carried by a draft but never used for retrieval. */
export function knowledgeExcludedFields(draft: KnowledgeQueryDraft): KnowledgeExcludedField[] {
  const excluded: KnowledgeExcludedField[] = [];
  if (draft.action != null) excluded.push({ field: 'do', value: draft.action });
  if (draft.context != null && draft.context.length > 0)
    excluded.push({ field: 'to', value: draft.context });
  for (const constraint of draft.constraints ?? [])
    excluded.push({ field: 'constraint', value: constraint });
  if (draft.format != null) excluded.push({ field: 'format', value: draft.format });
  if (draft.depth != null) excluded.push({ field: 'depth', value: draft.depth });
  return excluded;
}

function closestFieldName(name: string): string | undefined {
  return FIELD_NAMES.find((candidate) => candidate.startsWith(name.slice(0, 2)));
}

/**
 * Deterministically interprets composer or DSL text. Explicit `field:` syntax
 * yields `explicit`; a bare sentence is read as a subject and yields
 * `deterministic`; a recognized natural-language need keyword that could also
 * be an action is recorded as an ambiguity rather than discarded.
 */
export function parseMockKnowledgeQuery(text: string): KnowledgeQueryInterpretation {
  const diagnostics: KnowledgeDiagnostic[] = [];
  const ambiguities: KnowledgeParseAmbiguity[] = [];
  const { occurrences, unknownFields, leading } = fieldOccurrences(text);

  for (const unknown of unknownFields) {
    const suggestion = closestFieldName(unknown.name);
    diagnostics.push({
      code: 'unknownField',
      severity: 'error',
      message: `Unknown field "${unknown.name}".`,
      start: unknown.start,
      end: unknown.start + unknown.name.length,
      suggestion: suggestion ?? null,
    });
  }
  if (hasUnterminatedQuote(text)) {
    diagnostics.push({
      code: 'unterminatedQuote',
      severity: 'error',
      message: 'Unterminated quote.',
      start: text.indexOf('"'),
      end: text.length,
      suggestion: null,
    });
  }

  const about: string[] = [];
  const needs: KnowledgeNeed[] = [];
  const related: string[] = [];
  const scopes: KnowledgeScopeSelector[] = [];
  const constraints: string[] = [];
  let action: KnowledgeQueryDraft['action'];
  let context: string | undefined;
  let format: KnowledgeQueryDraft['format'];
  let depth: KnowledgeQueryDraft['depth'];
  const singleValueSeen = new Set<FieldName>();

  if (leading.length > 0 && occurrences.length > 0) about.push(leading);

  for (const occurrence of occurrences) {
    if (occurrence.values.length === 0) {
      diagnostics.push({
        code: 'emptyValue',
        severity: 'warning',
        message: `Field "${occurrence.field}" has no value.`,
        start: occurrence.start,
        end: occurrence.end,
        suggestion: null,
      });
      continue;
    }
    switch (occurrence.field) {
      case 'about':
        about.push(...occurrence.values);
        break;
      case 'need':
        for (const value of occurrence.values) {
          const need = NEED_ALIASES[normalizeKey(value)];
          if (need === undefined) {
            diagnostics.push({
              code: 'invalidNeedValue',
              severity: 'error',
              message: `Unknown need "${value}".`,
              start: occurrence.start,
              end: occurrence.end,
              suggestion:
                KNOWLEDGE_NEEDS.find((candidate) =>
                  candidate.startsWith(value.slice(0, 3).toLowerCase()),
                ) ?? null,
            });
            continue;
          }
          if (!needs.includes(need)) needs.push(need);
        }
        break;
      case 'related':
        related.push(...occurrence.values);
        break;
      case 'scope':
        for (const value of occurrence.values) {
          const selector = parseScopeSelector(value);
          if (selector === undefined) {
            diagnostics.push({
              code: 'invalidScopeValue',
              severity: 'error',
              message: `Unknown scope "${value}".`,
              start: occurrence.start,
              end: occurrence.end,
              suggestion: 'library',
            });
            continue;
          }
          scopes.push(selector);
        }
        break;
      case 'do': {
        const parsed = ACTION_ALIASES[normalizeKey(occurrence.values[0] ?? '')];
        if (parsed === undefined) {
          diagnostics.push({
            code: 'invalidActionValue',
            severity: 'error',
            message: `Unknown action "${occurrence.values[0] ?? ''}".`,
            start: occurrence.start,
            end: occurrence.end,
            suggestion: 'explain',
          });
          break;
        }
        if (singleValueSeen.has('do')) {
          diagnostics.push({
            code: 'duplicateField',
            severity: 'warning',
            message: 'Field "do" appears more than once; the last value wins.',
            start: occurrence.start,
            end: occurrence.end,
            suggestion: null,
          });
        }
        singleValueSeen.add('do');
        action = parsed;
        break;
      }
      case 'to':
        if (singleValueSeen.has('to')) {
          diagnostics.push({
            code: 'duplicateField',
            severity: 'warning',
            message: 'Field "to" appears more than once; the last value wins.',
            start: occurrence.start,
            end: occurrence.end,
            suggestion: null,
          });
        }
        singleValueSeen.add('to');
        context = occurrence.values.join(', ');
        break;
      case 'constraint':
        constraints.push(...occurrence.values);
        break;
      case 'format': {
        const parsed = FORMAT_ALIASES[normalizeKey(occurrence.values[0] ?? '')];
        if (parsed === undefined) {
          diagnostics.push({
            code: 'invalidFormatValue',
            severity: 'error',
            message: `Unknown format "${occurrence.values[0] ?? ''}".`,
            start: occurrence.start,
            end: occurrence.end,
            suggestion: 'narrative',
          });
          break;
        }
        format = parsed;
        break;
      }
      case 'depth': {
        const parsed = DEPTH_ALIASES[normalizeKey(occurrence.values[0] ?? '')];
        if (parsed === undefined) {
          diagnostics.push({
            code: 'invalidDepthValue',
            severity: 'error',
            message: `Unknown depth "${occurrence.values[0] ?? ''}".`,
            start: occurrence.start,
            end: occurrence.end,
            suggestion: 'standard',
          });
          break;
        }
        depth = parsed;
        break;
      }
    }
  }

  let confidence: KnowledgeParseConfidence = 'explicit';
  if (occurrences.length === 0) {
    const trimmed = text.trim();
    confidence = 'deterministic';
    if (trimmed.length > 0) {
      const words = trimmed.split(/\s+/u);
      const leadingWord = (words[0] ?? '').toLowerCase().replace(/[^a-z]/gu, '');
      const impliedNeed = NEED_ALIASES[leadingWord];
      const impliedAction = ACTION_ALIASES[leadingWord];
      if (impliedNeed !== undefined && impliedAction !== undefined) {
        confidence = 'ambiguous';
        needs.push(impliedNeed);
        about.push(words.slice(1).join(' ').trim());
        ambiguities.push({
          description: `"${leadingWord}" could be an information need or an answer action.`,
          alternativeNeed: impliedNeed,
          alternativeAction: impliedAction,
        });
      } else if (impliedNeed !== undefined && words.length > 1) {
        needs.push(impliedNeed);
        about.push(words.slice(1).join(' ').trim());
      } else if (impliedAction !== undefined && words.length > 1) {
        confidence = 'ambiguous';
        about.push(words.slice(1).join(' ').trim());
        ambiguities.push({
          description: `"${leadingWord}" was read as an answer action, not as retrieval text.`,
          alternativeNeed: null,
          alternativeAction: impliedAction,
        });
        action = impliedAction;
      } else {
        about.push(trimmed);
      }
    }
  }

  const draft: KnowledgeQueryDraft = {
    about: about.filter((value) => value.length > 0).slice(0, MAX_SUBJECTS),
    needs,
    related: related.slice(0, MAX_RELATED_TERMS),
    scopes,
    action: action ?? null,
    context: context ?? null,
    constraints,
    format: format ?? null,
    depth: depth ?? null,
  };

  return {
    draft,
    dslCompact: formatKnowledgeDraftCompact(draft),
    dslMultiline: formatKnowledgeDraftMultiline(draft),
    diagnostics,
    ambiguities,
    confidence,
    excludedFromRetrieval: knowledgeExcludedFields(draft),
  };
}

// --- Planning -------------------------------------------------------------

function scopeLabel(scope: KnowledgeScope): string {
  switch (scope.kind) {
    case 'entireLibrary':
      return 'Entire indexed library';
    case 'enrolledRoots':
      return `${scope.enrolledRootIds?.length ?? 0} indexed root(s)`;
    case 'currentFolder': {
      const uri = scope.folder?.uri ?? '';
      const label = uri.replace(/\/$/u, '').split('/').filter(Boolean).at(-1);
      return label === undefined || label.length === 0 ? uri : decodeURIComponent(label);
    }
    case 'semanticResults':
      return `${scope.semanticSourceIds?.length ?? 0} semantic result(s)`;
  }
}

function addSearch(
  searches: KnowledgePlannedSearch[],
  indexes: Map<string, number>,
  text: string,
  priority: KnowledgePlannedSearch['priority'],
  reason: KnowledgeSearchReason,
): void {
  const trimmed = text.trim();
  if (trimmed.length === 0) return;
  const key = trimmed.toLowerCase();
  const existing = indexes.get(key);
  if (existing !== undefined) {
    searches[existing]?.reasons.push(reason);
    return;
  }
  indexes.set(key, searches.length);
  searches.push({ text: trimmed, priority, reasons: [reason] });
}

/** Number of authorized sources the mock reports for a scope. */
function authorizedSources(scope: KnowledgeScope): number {
  return documentsInScope(scope).length;
}

/**
 * Largest scope the retrieval protocol can describe by exact source identity,
 * mirroring `MAX_ALLOWED_SOURCES` in `fm-semantic-worker::knowledge_retrieval`.
 */
const MAX_EXACT_SCOPE_SOURCES = 4_096;

/**
 * Whether retrieval ranked exactly the authorized scope, mirroring
 * `knowledge_service::partition_scope`.
 *
 * Library and root scopes are expressible as worker filters; folder and
 * semantic-result scopes are not, but they are still exact as long as their
 * authorized sources can be listed explicitly. Only a scope too large for that
 * is reported as a superset ranking.
 */
export function mockKnowledgeScopeIsExact(scope: KnowledgeScope): boolean {
  if (scope.kind === 'entireLibrary' || scope.kind === 'enrolledRoots') return true;
  return documentsInScope(scope).length <= MAX_EXACT_SCOPE_SOURCES;
}

/**
 * Whether one occurrence URI lies inside a folder URI, mirroring
 * `location_is_within_uri` in `fm-application::semantic_library`: a folder
 * contains itself and anything under a `/` boundary, never a sibling whose
 * name merely starts with the same characters.
 */
export function mockKnowledgeUriIsWithin(candidate: string, folder: string): boolean {
  const normalized = folder.replace(/\/+$/u, '');
  if (candidate === normalized) return true;
  return candidate.startsWith(`${normalized}/`);
}

/** Failure the mock raises for a scope the backend would also refuse. */
export class MockKnowledgeScopeError extends Error {
  constructor(
    readonly code: 'invalidRequest' | 'notFound',
    message: string,
  ) {
    super(message);
    this.name = 'MockKnowledgeScopeError';
  }
}

/**
 * Resolves the visible scope the way `knowledge_service::scope_selection` does,
 * including its promotion of DSL `scope: root:<id>` selectors when the visible
 * scope is the entire library and no root was picked in the composer.
 *
 * @throws MockKnowledgeScopeError when the backend would reject the scope.
 */
export function resolveMockKnowledgeScope(
  scope: KnowledgeScope,
  draft: KnowledgeQueryDraft,
): KnowledgeScope {
  const dslRootIds = (draft.scopes ?? [])
    .filter((selector) => selector.kind === 'root')
    .map((selector) => selector.id ?? '');
  const selectedRootIds = scope.enrolledRootIds ?? [];
  const rootIds =
    selectedRootIds.length === 0 && scope.kind === 'entireLibrary' ? dslRootIds : selectedRootIds;
  const resolved: KnowledgeScope =
    scope.kind === 'entireLibrary' && rootIds.length > 0
      ? { ...structuredClone(scope), kind: 'enrolledRoots', enrolledRootIds: rootIds }
      : structuredClone(scope);
  switch (resolved.kind) {
    case 'enrolledRoots': {
      if (rootIds.length === 0) {
        throw new MockKnowledgeScopeError(
          'invalidRequest',
          'enrolled-root knowledge scope is empty',
        );
      }
      const known = new Set(MOCK_KNOWLEDGE_ROOTS.map((root) => root.rootId));
      const unknown = rootIds.find((rootId) => !known.has(rootId));
      if (unknown !== undefined) {
        throw new MockKnowledgeScopeError('invalidRequest', `invalid knowledge root id ${unknown}`);
      }
      break;
    }
    case 'currentFolder':
      if (resolved.folder == null || resolved.folder.uri.length === 0) {
        throw new MockKnowledgeScopeError(
          'invalidRequest',
          'current-folder knowledge scope requires a folder',
        );
      }
      break;
    case 'semanticResults':
      if ((resolved.semanticSourceIds ?? []).length === 0) {
        throw new MockKnowledgeScopeError(
          'invalidRequest',
          'semantic-result knowledge scope is empty',
        );
      }
      break;
    case 'entireLibrary':
      break;
  }
  if (documentsInScope(resolved).length === 0) {
    throw new MockKnowledgeScopeError(
      'notFound',
      'knowledge scope contains no authorized indexed sources',
    );
  }
  return resolved;
}

/** Builds the deterministic plan for a draft, mirroring the canonical planner. */
export function planMockKnowledgeSearch(
  draft: KnowledgeQueryDraft,
  scope: KnowledgeScope,
  mode: KnowledgeRetrievalMode,
  options: KnowledgeSearchOptions,
): KnowledgeSearchPlan {
  const subjects = (draft.about ?? []).map((subject) => subject.trim()).filter(Boolean);
  const searches: KnowledgePlannedSearch[] = [];
  const indexes = new Map<string, number>();

  for (const [subjectIndex, subject] of subjects.entries()) {
    addSearch(searches, indexes, subject, 'primary', {
      kind: 'subject',
      subjectIndex,
      need: null,
      action: null,
      relatedTermIndex: null,
    });
  }

  const explicitNeeds = draft.needs ?? [];
  const action = draft.action ?? null;
  const needs =
    explicitNeeds.length > 0
      ? explicitNeeds
      : action === null
        ? []
        : (DEFAULT_ACTION_NEEDS[action] ?? []);
  for (const need of needs) {
    for (const [subjectIndex, subject] of subjects.entries()) {
      const usesActionDefault = explicitNeeds.length === 0 && action !== null;
      addSearch(searches, indexes, `${subject} ${need}`, 'secondary', {
        kind: usesActionDefault ? 'actionDefault' : 'need',
        subjectIndex,
        need,
        action: usesActionDefault ? action : null,
        relatedTermIndex: null,
      });
    }
  }

  for (const [relatedTermIndex, term] of (draft.related ?? []).entries()) {
    addSearch(searches, indexes, term, 'related', {
      kind: 'relatedTerm',
      subjectIndex: null,
      need: null,
      action: null,
      relatedTermIndex,
    });
  }

  const omittedSearches = Math.max(0, searches.length - options.maximumSearches);
  searches.length = Math.min(searches.length, options.maximumSearches);

  return {
    version: MOCK_KNOWLEDGE_PLANNER_VERSION,
    subjects,
    scope: structuredClone(scope),
    scopeLabel: scopeLabel(scope),
    scopeIsExact: mockKnowledgeScopeIsExact(scope),
    authorizedSources: authorizedSources(scope),
    mode,
    options: { ...options },
    searches,
    omittedSearches,
    excludedFromRetrieval: knowledgeExcludedFields(draft),
  };
}

// --- Execution ------------------------------------------------------------

function documentsInScope(scope: KnowledgeScope): readonly MockKnowledgeDocument[] {
  switch (scope.kind) {
    case 'enrolledRoots': {
      const ids = new Set(scope.enrolledRootIds ?? []);
      return MOCK_KNOWLEDGE_CORPUS.filter((document) => ids.has(document.rootId));
    }
    case 'semanticResults': {
      const ids = new Set(scope.semanticSourceIds ?? []);
      return MOCK_KNOWLEDGE_CORPUS.filter((document) => ids.has(document.sourceId));
    }
    case 'currentFolder': {
      const folder = scope.folder;
      if (folder == null) return [];
      return MOCK_KNOWLEDGE_CORPUS.filter(
        (document) =>
          document.providerId === folder.providerId &&
          mockKnowledgeUriIsWithin(document.uri, folder.uri),
      );
    }
    default:
      return MOCK_KNOWLEDGE_CORPUS;
  }
}

interface Candidate {
  readonly document: MockKnowledgeDocument;
  readonly chunk: MockKnowledgeChunk;
  readonly score: number;
}

function lexicalScore(chunk: MockKnowledgeChunk, query: string): number {
  const haystack = chunk.content.toLowerCase();
  const terms = query
    .toLowerCase()
    .split(/[^a-z0-9]+/u)
    .filter((term) => term.length > 2);
  if (terms.length === 0) return 0;
  return terms.reduce((total, term) => (haystack.includes(term) ? total + 1 : total), 0);
}

function routeOutcome(
  mode: KnowledgeRetrievalMode,
  capabilities: KnowledgeCapabilities,
): KnowledgeRouteOutcome {
  if (mode === 'fullText') {
    return { requested: 'fullText', applied: 'fullText', fallbackReason: null };
  }
  if (mode === 'semantic') {
    return { requested: 'semantic', applied: 'semantic', fallbackReason: null };
  }
  return capabilities.semantic
    ? { requested: 'hybrid', applied: 'hybrid', fallbackReason: null }
    : {
        requested: 'hybrid',
        applied: 'fullText',
        fallbackReason: 'queryEmbeddingsUnavailable',
      };
}

/**
 * Whether the requested route cannot run at all. A semantic-only request
 * without query embeddings fails rather than silently becoming a full-text
 * search, exactly as the worker's route selection does.
 */
export function mockKnowledgeRouteUnavailable(mode: KnowledgeRetrievalMode): boolean {
  const capabilities = mockKnowledgeCapabilities();
  if (mode === 'semantic') return !capabilities.semantic;
  if (mode === 'fullText') return !capabilities.fullText;
  return !capabilities.semantic && !capabilities.fullText;
}

/** Deterministic search result for one plan; nothing here calls a model. */
export function executeMockKnowledgeSearch(
  requestId: string,
  plan: KnowledgeSearchPlan,
): KnowledgeSearchResult {
  const capabilities = mockKnowledgeCapabilities();
  const documents = documentsInScope(plan.scope);
  const fused = new Map<
    string,
    {
      readonly document: MockKnowledgeDocument;
      readonly chunk: MockKnowledgeChunk;
      score: number;
      readonly contributions: KnowledgeRankContribution[];
      readonly matchedSearchIndexes: number[];
      readonly reasons: KnowledgeSearchReason[];
    }
  >();
  const traced: KnowledgeTracedQuery[] = [];

  for (const [searchIndex, search] of plan.searches.entries()) {
    const candidates: Candidate[] = [];
    for (const document of documents) {
      for (const chunk of document.chunks) {
        const score = lexicalScore(chunk, search.text);
        if (score > 0) candidates.push({ document, chunk, score });
      }
    }
    candidates.sort(
      (left, right) =>
        right.score - left.score ||
        left.document.documentId.localeCompare(right.document.documentId) ||
        left.chunk.sourcePosition - right.chunk.sourcePosition,
    );
    const limited = candidates.slice(0, plan.options.candidateLimit);
    traced.push({
      text: search.text,
      fullTextCandidates: limited.length,
      semanticCandidates: 0,
    });
    for (const [position, candidate] of limited.entries()) {
      const rank = position + 1;
      const existing = fused.get(candidate.chunk.recordId);
      const contribution: KnowledgeRankContribution = { route: 'fullText', rank, searchIndex };
      if (existing === undefined) {
        fused.set(candidate.chunk.recordId, {
          document: candidate.document,
          chunk: candidate.chunk,
          score: 1 / (RANK_CONSTANT + rank),
          contributions: [contribution],
          matchedSearchIndexes: [searchIndex],
          reasons: [...search.reasons],
        });
        continue;
      }
      existing.score += 1 / (RANK_CONSTANT + rank);
      existing.contributions.push(contribution);
      if (!existing.matchedSearchIndexes.includes(searchIndex))
        existing.matchedSearchIndexes.push(searchIndex);
      existing.reasons.push(...search.reasons);
    }
  }

  const ranked = [...fused.values()].sort(
    (left, right) =>
      right.score - left.score ||
      left.document.documentId.localeCompare(right.document.documentId) ||
      left.chunk.sourcePosition - right.chunk.sourcePosition,
  );

  const perDocument = new Map<string, number>();
  const evidence: KnowledgeEvidence[] = [];
  let tokenCount = 0;
  for (const row of ranked) {
    if (evidence.length >= plan.options.resultLimit) break;
    const used = perDocument.get(row.document.documentId) ?? 0;
    if (used >= plan.options.maximumResultsPerFile) continue;
    perDocument.set(row.document.documentId, used + 1);
    const tokens = row.chunk.content.split(/\s+/u).length;
    if (tokenCount + tokens > plan.options.contextTokenBudget) break;
    tokenCount += tokens;
    evidence.push({
      recordId: row.chunk.recordId,
      documentId: row.document.documentId,
      sourceId: row.document.sourceId,
      duplicateSourceIds: [],
      title: row.document.title,
      excerpt: row.chunk.content.slice(0, 160),
      content: row.chunk.content,
      sectionPath: [...row.chunk.sectionPath],
      provenance: JSON.stringify({ kind: 'textLines', start_line: 1, end_line: 4 }),
      chunkKind: 'chunk',
      sourcePosition: row.chunk.sourcePosition,
      adjacent: false,
      generated: false,
      stale: row.document.stale,
      unavailable: row.document.unavailable,
      mediaType: row.document.mediaType,
      modifiedAtMs: row.document.modifiedAtMs,
      tokenCount: tokens,
      fusedScore: Number(row.score.toFixed(6)),
      finalRank: evidence.length + 1,
      matchedSearchIndexes: [...row.matchedSearchIndexes],
      rankContributions: row.contributions,
      reasons: row.reasons,
    });
  }

  const staleEvidence = evidence.filter((item) => item.stale === true).length;
  const unknownFreshnessEvidence = evidence.filter((item) => item.stale == null).length;
  const unavailableEvidence = evidence.filter((item) => item.unavailable).length;
  const eligible = documents.length;
  // Every document in the mock corpus has a published generation; what the mock
  // does *not* always know is whether the indexed content is still current,
  // which is reported through `fingerprinted`/`staleEvidence` instead.
  const indexed = eligible;
  const fingerprinted = documents.filter((document) => document.stale !== null).length;
  const unavailable = documents.filter((document) => document.unavailable).length;

  return {
    requestId,
    plan,
    capabilities,
    route: routeOutcome(plan.mode, capabilities),
    evidence,
    evidenceFingerprint: `mock-knowledge-${plan.searches.map((search) => search.text).join('|')}`,
    tokenCount,
    withheldUnauthorized: 0,
    coverage: {
      eligible,
      indexed,
      fingerprinted,
      partial: mockKnowledgeCoverageIsPartial({
        eligible,
        indexed,
        unavailable,
        staleEvidence,
        scopeIsExact: plan.scopeIsExact,
      }),
      scopeIsExact: plan.scopeIsExact,
      staleEvidence,
      unavailable,
      unavailableEvidence,
      unknownFreshnessEvidence,
    },
    trace: plan.options.includeTrace ? { queries: traced, rankConstant: RANK_CONSTANT } : null,
  };
}

/**
 * Whether a scope is *known* to be partially represented, mirroring
 * `KnowledgeSearchCoverage::partial` in `fm-application::knowledge_search`.
 *
 * A smaller scope is not partial coverage: what makes coverage partial is an
 * inexact ranking scope, a source that cannot be opened, evidence that changed
 * since indexing, or a known publication gap. Unknown publication state is
 * reported through `indexed` instead of being folded in here.
 */
export function mockKnowledgeCoverageIsPartial(coverage: {
  readonly eligible: number;
  readonly indexed: number | null;
  readonly unavailable: number;
  readonly staleEvidence: number;
  readonly scopeIsExact: boolean;
}): boolean {
  return (
    !coverage.scopeIsExact ||
    coverage.unavailable !== 0 ||
    coverage.staleEvidence !== 0 ||
    (coverage.indexed !== null && coverage.indexed < coverage.eligible)
  );
}

/** Resolves one opaque evidence source into its current navigable location. */
export function resolveMockKnowledgeSource(sourceId: string): KnowledgeSourceLocation | undefined {
  const document = MOCK_KNOWLEDGE_CORPUS.find((candidate) => candidate.sourceId === sourceId);
  if (document === undefined) return undefined;
  return {
    entryId: document.uri,
    location: { providerId: document.providerId, uri: document.uri },
    available: !document.unavailable,
  };
}
