import m, { type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import {
  cornerDownLeftIcon,
  externalLinkIcon,
  filterIcon,
  settingsIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  KnowledgeAnswer,
  KnowledgeAnswerCitation,
  KnowledgeCapabilities,
  KnowledgeDiagnostic,
  KnowledgeEvidence,
  KnowledgeNeed,
  KnowledgeQueryDraft,
  KnowledgeQueryInterpretation,
  KnowledgeRetrievalMode,
  KnowledgeRoot,
  KnowledgeRouteFallbackReason,
  KnowledgeScope,
  KnowledgeScopeKind,
  KnowledgeScopeSelector,
  KnowledgeSearchOptions,
  KnowledgeSearchPlan,
  KnowledgeSearchReason,
  KnowledgeSearchResult,
  LlmEndpointLocality,
  LlmProfile,
  Location,
} from '../../models';
import { defaultKnowledgeSearchOptions } from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { decodeEvidenceTitle } from './evidence-title';

/** Everything the shell knows about the default scope when the dialog opens. */
export interface KnowledgeSearchDialogAttrs {
  readonly open: boolean;
  readonly client: FileManagerClient;
  readonly workspaceId: string;
  /** Active directory, offered as the default scope when it is indexed. */
  readonly currentFolder: Location | undefined;
  /** Opaque source ids of the active semantic result set, if any. */
  readonly semanticSourceIds: readonly string[];
  /** Initial subject text, e.g. the active quick filter or semantic query. */
  readonly initialSubject?: string | undefined;
  readonly onClose: () => void;
  readonly onOpenSource?: (evidence: KnowledgeEvidence) => void | Promise<void>;
}

/** Canonical need order, matching the backend enum declaration order. */
const NEEDS: readonly KnowledgeNeed[] = [
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

const MODES: readonly KnowledgeRetrievalMode[] = ['hybrid', 'fullText', 'semantic'];

const SCOPE_KINDS: readonly KnowledgeScopeKind[] = [
  'entireLibrary',
  'enrolledRoots',
  'currentFolder',
  'semanticResults',
];

const PREFERENCES_STORAGE_KEY = 'procyon.knowledgeSearch.preferences.v1';

function loadPreferredNeeds(): KnowledgeNeed[] {
  try {
    const stored = globalThis.localStorage?.getItem(PREFERENCES_STORAGE_KEY);
    if (stored == null) return [];
    const value: unknown = JSON.parse(stored);
    if (
      typeof value !== 'object' ||
      value === null ||
      !('needs' in value) ||
      !Array.isArray(value.needs)
    ) {
      return [];
    }
    const selected = new Set(
      value.needs.filter(
        (need): need is KnowledgeNeed =>
          typeof need === 'string' && NEEDS.includes(need as KnowledgeNeed),
      ),
    );
    return NEEDS.filter((need) => selected.has(need));
  } catch {
    return [];
  }
}

function persistPreferredNeeds(needs: readonly KnowledgeNeed[]): void {
  try {
    globalThis.localStorage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify({ needs }));
  } catch {
    // Search remains usable when a host disables persistent browser storage.
  }
}

function needLabel(need: KnowledgeNeed): string {
  switch (need) {
    case 'overview':
      return t('knowledgeSearch', 'needOverview');
    case 'definition':
      return t('knowledgeSearch', 'needDefinition');
    case 'procedure':
      return t('knowledgeSearch', 'needProcedure');
    case 'examples':
      return t('knowledgeSearch', 'needExamples');
    case 'evidence':
      return t('knowledgeSearch', 'needEvidence');
    case 'arguments':
      return t('knowledgeSearch', 'needArguments');
    case 'comparison':
      return t('knowledgeSearch', 'needComparison');
    case 'limitations':
      return t('knowledgeSearch', 'needLimitations');
    case 'references':
      return t('knowledgeSearch', 'needReferences');
  }
}

function modeLabel(mode: KnowledgeRetrievalMode): string {
  switch (mode) {
    case 'hybrid':
      return t('knowledgeSearch', 'modeHybrid');
    case 'fullText':
      return t('knowledgeSearch', 'modeFullText');
    case 'semantic':
      return t('knowledgeSearch', 'modeSemantic');
  }
}

function scopeKindLabel(kind: KnowledgeScopeKind): string {
  switch (kind) {
    case 'entireLibrary':
      return t('knowledgeSearch', 'scopeEntireLibrary');
    case 'enrolledRoots':
      return t('knowledgeSearch', 'scopeEnrolledRoots');
    case 'currentFolder':
      return t('knowledgeSearch', 'scopeCurrentFolder');
    case 'semanticResults':
      return t('knowledgeSearch', 'scopeSemanticResults');
  }
}

function fallbackReasonLabel(reason: KnowledgeRouteFallbackReason): string {
  switch (reason) {
    case 'queryEmbeddingsUnavailable':
      return t('knowledgeSearch', 'fallbackQueryEmbeddingsUnavailable');
    case 'queryEmbeddingFailed':
      return t('knowledgeSearch', 'fallbackQueryEmbeddingFailed');
    case 'semanticQueryFailed':
      return t('knowledgeSearch', 'fallbackSemanticQueryFailed');
    case 'fullTextIndexUnavailable':
      return t('knowledgeSearch', 'fallbackFullTextIndexUnavailable');
    case 'fullTextQueryFailed':
      return t('knowledgeSearch', 'fallbackFullTextQueryFailed');
  }
}

function reasonLabel(reason: KnowledgeSearchReason): string {
  switch (reason.kind) {
    case 'subject':
      return t('knowledgeSearch', 'reasonSubject');
    case 'need':
      return t('knowledgeSearch', 'reasonNeed', {
        need: reason.need == null ? '' : needLabel(reason.need),
      });
    case 'actionDefault':
      return t('knowledgeSearch', 'reasonActionDefault', {
        action: reason.action ?? '',
        need: reason.need == null ? '' : needLabel(reason.need),
      });
    case 'relatedTerm':
      return t('knowledgeSearch', 'reasonRelatedTerm');
  }
}

function priorityLabel(priority: KnowledgeSearchPlan['searches'][number]['priority']): string {
  switch (priority) {
    case 'primary':
      return t('knowledgeSearch', 'planSearchPrimary');
    case 'secondary':
      return t('knowledgeSearch', 'planSearchSecondary');
    case 'related':
      return t('knowledgeSearch', 'planSearchRelated');
  }
}

/** Renders serialized structural provenance as a short human location. */
export function knowledgeProvenanceLabel(provenance: string): string {
  const describe = (value: unknown): string | undefined => {
    if (typeof value === 'string') {
      try {
        return describe(JSON.parse(value));
      } catch {
        return value.length === 0 ? undefined : value;
      }
    }
    if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined;
    const node = value as Record<string, unknown>;
    const number = (key: string): number | undefined =>
      typeof node[key] === 'number' ? node[key] : undefined;
    switch (node.kind) {
      case 'exact':
        return describe(node.value);
      case 'span': {
        const span =
          node.value !== null && typeof node.value === 'object' && !Array.isArray(node.value)
            ? (node.value as Record<string, unknown>)
            : node;
        const first = describe(span.first);
        const last = describe(span.last);
        if (first === undefined) return last;
        if (last === undefined || last === first) return first;
        return `${first} – ${last}`;
      }
      case 'pdfBlock': {
        const page = number('page_number');
        return page === undefined ? undefined : t('ragAsk', 'citationPage', { page });
      }
      case 'textLines':
      case 'codeLines': {
        const start = number('start_line');
        const end = number('end_line');
        return start === undefined || end === undefined
          ? undefined
          : t('ragAsk', 'citationLines', { start, end });
      }
      case 'epubText': {
        const spine = number('spine_index');
        const start = number('start_line');
        const end = number('end_line');
        return spine === undefined || start === undefined || end === undefined
          ? undefined
          : t('ragAsk', 'citationEpubLines', { chapter: spine + 1, start, end });
      }
      case 'slide': {
        const slide = number('slide_number');
        return slide === undefined ? undefined : t('ragAsk', 'citationSlide', { slide });
      }
      case 'spreadsheetRange': {
        const range = spreadsheetRangeLabel(node);
        if (range === undefined) return undefined;
        const sheet = typeof node.sheet === 'string' ? node.sheet.trim() : '';
        return sheet.length === 0
          ? t('ragAsk', 'citationCells', { range })
          : t('ragAsk', 'citationSheetCells', { sheet, range });
      }
      case 'docxBlock': {
        const block = number('block_index');
        return block === undefined ? undefined : t('ragAsk', 'citationBlock', { block: block + 1 });
      }
      default:
        return undefined;
    }
  };
  return describe(provenance) ?? '';
}

/**
 * Renders a 0-based inclusive spreadsheet cell rectangle as A1 notation. A
 * single cell renders as `B3`, a rectangle as `B3:D8`; anything with a missing
 * or non-numeric bound renders as nothing rather than as an invented range.
 */
function spreadsheetRangeLabel(node: Record<string, unknown>): string | undefined {
  const index = (key: string): number | undefined => {
    const value = node[key];
    return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : undefined;
  };
  const startRow = index('start_row');
  const startColumn = index('start_column');
  const endRow = index('end_row');
  const endColumn = index('end_column');
  if (
    startRow === undefined ||
    startColumn === undefined ||
    endRow === undefined ||
    endColumn === undefined
  ) {
    return undefined;
  }
  const first = `${spreadsheetColumnName(startColumn)}${startRow + 1}`;
  const last = `${spreadsheetColumnName(endColumn)}${endRow + 1}`;
  return first === last ? first : `${first}:${last}`;
}

/** Converts a 0-based column index into its spreadsheet letters (0 → `A`). */
function spreadsheetColumnName(column: number): string {
  let remaining = column;
  let name = '';
  do {
    name = String.fromCharCode(65 + (remaining % 26)) + name;
    remaining = Math.floor(remaining / 26) - 1;
  } while (remaining >= 0);
  return name;
}

/** Canonical DSL text for one scope selector, matching the backend serialiser. */
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

/** Quotes a value exactly like the DSL serialiser, so a round-trip is lossless. */
function quoteValue(value: string): string {
  return /[\s,:;"]/u.test(value) ? `"${value.replace(/(["\\])/gu, '\\$1')}"` : value;
}

/** Serialises a value list into the comma-separated DSL form. */
function joinValues(values: readonly string[]): string {
  return values.map(quoteValue).join(', ');
}

/**
 * Splits a comma-separated DSL value list, honouring quotes and escapes, so a
 * value that itself contains a comma - `"ACME, Inc."` - stays one value.
 */
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

/**
 * Reads the subject editor: one subject per line or comma-separated, using the
 * same quoting rules as the DSL, so a subject like `"ACME, Inc."` survives
 * editing as the single value the parser returned.
 */
function splitSubjects(value: string): string[] {
  return value.split('\n').flatMap((line) => splitValues(line));
}

/** Writes the subject editor: one quoted-as-needed subject per line. */
function joinSubjects(values: readonly string[]): string {
  return values.map(quoteValue).join('\n');
}

/**
 * Ranks documents by their matched evidence while restoring each document's
 * structural order. Adjacent chunks remain available to grounded-answer
 * generation, but are not presented as search matches. Two documents can
 * share a title, so the stable `documentId` remains the key.
 */
export function groupEvidenceByDocument(evidence: readonly KnowledgeEvidence[]): readonly {
  readonly documentId: string;
  readonly title: string;
  readonly rows: KnowledgeEvidence[];
  readonly openEvidence: KnowledgeEvidence;
  readonly bestRank: number;
  readonly bestScore: number;
}[] {
  const groups = new Map<
    string,
    {
      documentId: string;
      title: string;
      rows: KnowledgeEvidence[];
      openEvidence: KnowledgeEvidence;
      bestRank: number;
      bestScore: number;
    }
  >();
  for (const row of evidence) {
    if (row.adjacent) continue;
    const existing = groups.get(row.documentId);
    if (existing === undefined) {
      groups.set(row.documentId, {
        documentId: row.documentId,
        title: decodeEvidenceTitle(row.title) ?? row.title,
        rows: [row],
        openEvidence: row,
        bestRank: row.finalRank,
        bestScore: row.fusedScore,
      });
      continue;
    }
    existing.rows.push(row);
    const outranksBest =
      row.finalRank < existing.bestRank ||
      (row.finalRank === existing.bestRank && row.fusedScore > existing.bestScore);
    if (outranksBest) {
      existing.bestRank = row.finalRank;
      existing.bestScore = row.fusedScore;
    }
    const outranksOpenTarget =
      row.finalRank < existing.openEvidence.finalRank ||
      (row.finalRank === existing.openEvidence.finalRank &&
        row.fusedScore > existing.openEvidence.fusedScore);
    if (
      (!row.unavailable && existing.openEvidence.unavailable) ||
      (row.unavailable === existing.openEvidence.unavailable && outranksOpenTarget)
    ) {
      existing.openEvidence = row;
    }
  }
  return [...groups.values()]
    .map((group) => ({
      ...group,
      rows: group.rows.toSorted(
        (left, right) =>
          left.sourcePosition - right.sourcePosition ||
          left.finalRank - right.finalRank ||
          left.recordId.localeCompare(right.recordId),
      ),
    }))
    .toSorted(
      (left, right) =>
        left.bestRank - right.bestRank ||
        right.bestScore - left.bestScore ||
        left.documentId.localeCompare(right.documentId),
    );
}

/** An emptied canonical draft; every field is explicit so nothing is inferred. */
function emptyDraft(): KnowledgeQueryDraft {
  return {
    about: [],
    needs: [],
    related: [],
    scopes: [],
    action: null,
    context: null,
    constraints: [],
    format: null,
    depth: null,
  };
}

/** Why one DSL scope selector cannot be applied to the visual scope control. */
export interface KnowledgeScopeIssue {
  readonly selector: KnowledgeScopeSelector;
  readonly reason: 'unknownRoot' | 'unsupported';
}

/**
 * Splits DSL scope selectors into the ones the visual scope control can
 * represent and the ones it cannot.
 *
 * `library` maps to the entire-library scope and `root:<id>` to a selected
 * indexed root; a root the host never reported, and any `workspace:<id>`
 * selector (which the backend resolves against a different authority), are
 * returned as issues so the dialog can say so instead of dropping them.
 */
export function classifyKnowledgeScopeSelectors(
  selectors: readonly KnowledgeScopeSelector[],
  knownRootIds: ReadonlySet<string>,
): {
  readonly wholeLibrary: boolean;
  readonly rootIds: readonly string[];
  readonly issues: readonly KnowledgeScopeIssue[];
} {
  const rootIds: string[] = [];
  const issues: KnowledgeScopeIssue[] = [];
  let wholeLibrary = false;
  for (const selector of selectors) {
    switch (selector.kind) {
      case 'wholeLibrary':
        wholeLibrary = true;
        break;
      case 'root': {
        const id = selector.id ?? '';
        if (knownRootIds.has(id)) rootIds.push(id);
        else issues.push({ selector, reason: 'unknownRoot' });
        break;
      }
      case 'workspace':
        issues.push({ selector, reason: 'unsupported' });
        break;
    }
  }
  return { wholeLibrary, rootIds, issues };
}

/** Anchor prefix for a citation link inside generated answer markdown. */
const CITATION_ANCHOR = '#fm-knowledge-citation-';

/** Escapes a citation label for use inside a regular expression. */
function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}

/**
 * Turns the opaque citation labels a model copied - `[E1]`, `(E1, E2)` - into
 * in-document links.
 *
 * Only the citations passed in are linked, and the caller passes only those
 * whose identity matches a row the search actually displayed, so a label the
 * model invented stays inert text rather than becoming something clickable.
 */
export function linkKnowledgeCitations(
  markdown: string,
  citations: readonly KnowledgeAnswerCitation[],
): string {
  return citations.reduce((linked, citation) => {
    const label = escapeRegExp(citation.label);
    const target = `${CITATION_ANCHOR}${encodeURIComponent(citation.label)}`;
    return linked
      .replace(new RegExp(`\\[${label}\\](?!\\()`, 'gu'), `[${citation.label}](${target})`)
      .replace(new RegExp(`\\(${label}(?=[,\\s)])`, 'gu'), `([${citation.label}](${target})`);
  }, markdown);
}

/**
 * Whether a host refused to answer because the inspected evidence set is gone.
 *
 * Every host reports this the same way - an `ApiError`, a Tauri
 * `ApplicationErrorDto`, or the mock's `MockClientError` all carry the typed
 * `code` - so the dialog can tell the user to search again instead of guessing
 * from a message, and never re-searches on their behalf.
 */
export function isKnowledgeEvidenceRefreshRequired(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  return (error as { readonly code?: unknown }).code === 'knowledgeEvidenceRefreshRequired';
}

/** Maps transport-stable error codes to actionable, localized search guidance. */
export function knowledgeSearchErrorMessage(error: unknown): string {
  const code =
    typeof error === 'object' && error !== null
      ? (error as { readonly code?: unknown }).code
      : undefined;
  switch (code) {
    case 'notFound':
      return t('knowledgeSearch', 'searchNoSources');
    case 'providerUnavailable':
      return t('knowledgeSearch', 'searchUnavailable');
    case 'permissionDenied':
      return t('knowledgeSearch', 'searchDenied');
    case 'invalidRequest':
      return t('knowledgeSearch', 'searchInvalid');
    case 'operationCancelled':
      return t('knowledgeSearch', 'cancelled');
    default:
      return t('knowledgeSearch', 'searchFailed');
  }
}

export const KnowledgeSearchPane: FactoryComponent<KnowledgeSearchDialogAttrs> = () => {
  let wasOpen = false;
  let busy: 'loading' | 'planning' | 'searching' | undefined;
  let error: string | undefined;
  let notice: string | undefined;
  let capabilities: KnowledgeCapabilities | undefined;
  let roots: readonly KnowledgeRoot[] = [];
  /**
   * Canonical, lossless query state. The visual controls and the DSL box are
   * two editors over *this* value, never two independent sources of truth, so a
   * quoted subject such as `about: "ACME, Inc."` stays one element no matter
   * which editor last touched it.
   */
  let draft: KnowledgeQueryDraft = emptyDraft();
  let subjectsText = '';
  let mode: KnowledgeRetrievalMode = 'hybrid';
  let scopeKind: KnowledgeScopeKind = 'entireLibrary';
  let selectedRootIds = new Set<string>();
  let dslText = '';
  let includeTrace = false;
  let currentFolderIndexed = false;
  let interpretation: KnowledgeQueryInterpretation | undefined;
  /** DSL scopes that cannot be applied; search stays blocked while any exist. */
  let scopeIssues: readonly KnowledgeScopeIssue[] = [];
  /** Whether the query explicitly declared the whole-library scope selector. */
  let wholeLibraryDeclared = false;
  let plan: KnowledgeSearchPlan | undefined;
  let result: KnowledgeSearchResult | undefined;
  let traceRequested = false;
  let abortController: AbortController | undefined;
  /**
   * Answer state (task 0207). Everything here is *downstream* of a completed
   * search: it exists only while a successful result whose capabilities report
   * `answerGeneration` is on screen, and it never influences retrieval.
   */
  let answerProfiles: readonly LlmProfile[] = [];
  let answerProfilesFailed = false;
  let selectedAnswerProfileId = '';
  /** Grounded-only by default; the opt-in is explicit and labelled. */
  let allowModelKnowledge = false;
  let answer: KnowledgeAnswer | undefined;
  let answerError: string | undefined;
  let answerNotice: string | undefined;
  let generatingAnswer = false;
  let answerAbortController: AbortController | undefined;
  /** Bumped per generation, so only the newest response owns the state. */
  let answerSequence = 0;
  /** Set when an answer arrived and its region should take focus once. */
  let focusAnswerOnReady = false;
  /** Bumped on every open/close, so responses from a previous life are dropped. */
  let generation = 0;
  /** Bumped on every edit, so a response for an older draft is never applied. */
  let revision = 0;
  /** In-flight interpretation the Enter handler must await before searching. */
  let pendingParse: Promise<void> | undefined;
  /** Set when the dialog must take focus after its next render. */
  let focusSubjectOnOpen = false;
  let settingsOpen = false;

  /** Bounded options sent with both the plan preview and the search itself. */
  function searchOptions(): KnowledgeSearchOptions {
    return { ...defaultKnowledgeSearchOptions(), includeTrace };
  }

  /** Scope selectors expressed by the visual scope control, plus what it cannot express. */
  function scopeSelectors(): KnowledgeScopeSelector[] {
    // Selectors the composer could not apply are carried through verbatim
    // rather than quietly deleted, so the DSL keeps saying what the user wrote
    // while the dialog explains why that query cannot run.
    const retained = scopeIssues.map((issue) => issue.selector);
    if (scopeKind === 'enrolledRoots') {
      return [
        ...roots
          .filter((root) => selectedRootIds.has(root.rootId))
          .map((root): KnowledgeScopeSelector => ({ kind: 'root', id: root.rootId })),
        ...retained,
      ];
    }
    if (scopeKind === 'entireLibrary' && wholeLibraryDeclared) {
      return [{ kind: 'wholeLibrary', id: null }, ...retained];
    }
    return retained;
  }

  /** The canonical draft as it is sent to the backend, with scopes projected. */
  function currentDraft(): KnowledgeQueryDraft {
    return { ...draft, scopes: scopeSelectors() };
  }

  /** Records one editor change: results are stale, and so is any in-flight response. */
  function edited(): void {
    revision += 1;
    resetResults();
  }

  function scope(attrs: KnowledgeSearchDialogAttrs): KnowledgeScope {
    return {
      workspaceId: attrs.workspaceId,
      kind: scopeKind,
      folder: scopeKind === 'currentFolder' ? (attrs.currentFolder ?? null) : null,
      enrolledRootIds: scopeKind === 'enrolledRoots' ? [...selectedRootIds] : [],
      semanticSourceIds: scopeKind === 'semanticResults' ? [...attrs.semanticSourceIds] : [],
    };
  }

  function scopeAvailable(attrs: KnowledgeSearchDialogAttrs, kind: KnowledgeScopeKind): boolean {
    switch (kind) {
      case 'entireLibrary':
        return true;
      case 'enrolledRoots':
        return roots.length > 0;
      case 'currentFolder':
        return attrs.currentFolder !== undefined && currentFolderIndexed;
      case 'semanticResults':
        return attrs.semanticSourceIds.length > 0;
    }
  }

  function modeAvailable(candidate: KnowledgeRetrievalMode): boolean {
    if (capabilities === undefined) return candidate === 'hybrid';
    switch (candidate) {
      case 'hybrid':
        return capabilities.fullText || capabilities.semantic;
      case 'fullText':
        return capabilities.fullText;
      case 'semantic':
        return capabilities.semantic;
    }
  }

  function resetResults(): void {
    result = undefined;
    plan = undefined;
    notice = undefined;
    traceRequested = false;
    resetAnswer();
  }

  /**
   * Drops any answer and stops one in flight. An answer describes one exact
   * evidence set, so an edit, a new search, a close or a reopen must leave
   * nothing of it behind - not even a response still on its way.
   */
  function resetAnswer(): void {
    answerAbortController?.abort();
    answerAbortController = undefined;
    answerSequence += 1;
    generatingAnswer = false;
    answer = undefined;
    answerError = undefined;
    answerNotice = undefined;
    focusAnswerOnReady = false;
  }

  /**
   * Applies one interpretation to the canonical draft and to whichever editor
   * did not produce it. Re-canonicalising the DSL box while the user is typing
   * in it would delete every half-finished field (`need: d` parses to nothing),
   * so a DSL-sourced interpretation only ever updates the visual controls.
   */
  function applyInterpretation(next: KnowledgeQueryInterpretation, fromDsl: boolean): void {
    interpretation = next;
    if (!fromDsl) {
      dslText = next.dslMultiline;
      return;
    }
    draft = { ...next.draft, scopes: [] };
    subjectsText = joinSubjects(next.draft.about ?? []);
    persistPreferredNeeds(next.draft.needs ?? []);
    applyScopeSelectors(next.draft.scopes ?? []);
  }

  /**
   * Synchronises DSL scope selectors with the visual scope control instead of
   * ignoring them, and records the ones that cannot be represented so the
   * dialog can report them and refuse to search.
   */
  function applyScopeSelectors(selectors: readonly KnowledgeScopeSelector[]): void {
    const classified = classifyKnowledgeScopeSelectors(
      selectors,
      new Set(roots.map((root) => root.rootId)),
    );
    scopeIssues = classified.issues;
    wholeLibraryDeclared = classified.wholeLibrary;
    if (classified.rootIds.length > 0) {
      scopeKind = 'enrolledRoots';
      selectedRootIds = new Set(classified.rootIds);
      return;
    }
    if (classified.wholeLibrary) {
      scopeKind = 'entireLibrary';
    }
  }

  /**
   * Re-interprets the current query so both editors stay equivalent. Responses
   * are applied only when they still belong to this dialog generation and to
   * the revision they were issued for, so an edit, a close or a reopen while a
   * parse is in flight discards it.
   */
  function reinterpret(attrs: KnowledgeSearchDialogAttrs, text?: string): Promise<void> {
    const parseGeneration = generation;
    const parseRevision = revision;
    const source = text ?? formatDraftText();
    if (source.trim().length === 0) {
      interpretation = undefined;
      scopeIssues = [];
      wholeLibraryDeclared = false;
      if (text === undefined) {
        dslText = '';
      } else {
        // Emptying the query language box empties the query, so the visual
        // composer cannot keep editing a draft the user just deleted.
        draft = emptyDraft();
        subjectsText = '';
      }
      pendingParse = undefined;
      return Promise.resolve();
    }
    const parse = attrs.client
      .parseKnowledgeQuery({ text: source })
      .then((parsed) => {
        if (parseGeneration !== generation || parseRevision !== revision) return;
        applyInterpretation(parsed, text !== undefined);
      })
      .catch(() => {
        if (parseGeneration !== generation || parseRevision !== revision) return;
        error = t('knowledgeSearch', 'parseFailed');
      })
      .finally(() => {
        if (parseGeneration !== generation || parseRevision !== revision) return;
        pendingParse = undefined;
        m.redraw();
      });
    pendingParse = parse;
    return parse;
  }

  /** Serialises the canonical draft into DSL text the parser accepts. */
  function formatDraftText(): string {
    const current = currentDraft();
    const lines: string[] = [];
    if ((current.about ?? []).length > 0) lines.push(`about: ${joinValues(current.about ?? [])}`);
    if ((current.needs ?? []).length > 0) lines.push(`need: ${(current.needs ?? []).join(', ')}`);
    if ((current.related ?? []).length > 0)
      lines.push(`related: ${joinValues(current.related ?? [])}`);
    if ((current.scopes ?? []).length > 0)
      lines.push(`scope: ${joinValues((current.scopes ?? []).map(scopeSelectorText))}`);
    if (current.action != null) lines.push(`do: ${current.action}`);
    if (current.context != null && current.context.length > 0)
      lines.push(`to: ${quoteValue(current.context)}`);
    for (const constraint of current.constraints ?? [])
      lines.push(`constraint: ${quoteValue(constraint)}`);
    if (current.format != null) lines.push(`format: ${current.format}`);
    if (current.depth != null) lines.push(`depth: ${current.depth}`);
    return lines.join('\n');
  }

  async function load(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    // A new generation invalidates every response still in flight from the
    // previous open, so none of them can be applied to this fresh composer.
    generation += 1;
    revision += 1;
    const loadGeneration = generation;
    pendingParse = undefined;
    busy = 'loading';
    error = undefined;
    notice = undefined;
    resetResults();
    interpretation = undefined;
    scopeIssues = [];
    wholeLibraryDeclared = false;
    draft = {
      ...emptyDraft(),
      about: splitSubjects(attrs.initialSubject ?? ''),
      needs: loadPreferredNeeds(),
    };
    subjectsText = attrs.initialSubject ?? '';
    dslText = '';
    currentFolderIndexed = false;
    includeTrace = false;
    capabilities = undefined;
    roots = [];
    selectedRootIds = new Set();
    mode = 'hybrid';
    scopeKind = 'entireLibrary';
    focusSubjectOnOpen = true;
    answerProfiles = [];
    answerProfilesFailed = false;
    selectedAnswerProfileId = '';
    allowModelKnowledge = false;
    settingsOpen = false;
    try {
      const [reportedCapabilities, availableRoots] = await Promise.all([
        attrs.client.getKnowledgeCapabilities(),
        attrs.client.listKnowledgeRoots({ workspaceId: attrs.workspaceId }),
      ]);
      if (loadGeneration !== generation) return;
      capabilities = reportedCapabilities;
      roots = availableRoots;
      selectedRootIds = new Set(
        availableRoots.filter((root) => root.available).map((r) => r.rootId),
      );
      mode = reportedCapabilities.semantic
        ? 'hybrid'
        : reportedCapabilities.fullText
          ? 'hybrid'
          : 'fullText';
      // Generation profiles are only fetched when the host actually offers
      // answers, so a search-only host is never asked about a capability it
      // does not have, and a failure here never blocks search (task 0207).
      if (reportedCapabilities.answerGeneration) {
        try {
          const profiles = await attrs.client.listLlmProfiles();
          if (loadGeneration !== generation) return;
          answerProfiles = profiles;
        } catch {
          if (loadGeneration !== generation) return;
          answerProfilesFailed = true;
        }
      }
      if (attrs.currentFolder !== undefined) {
        try {
          const folder = await attrs.client.getSemanticFolderStatus({
            workspaceId: attrs.workspaceId,
            location: attrs.currentFolder,
          });
          if (loadGeneration !== generation) return;
          currentFolderIndexed =
            (folder.consent === 'includedHere' || folder.consent === 'inheritedFromParent') &&
            folder.workspaceReferenced;
        } catch {
          currentFolderIndexed = false;
        }
      }
      scopeKind = scopeAvailable(attrs, 'currentFolder')
        ? 'currentFolder'
        : scopeAvailable(attrs, 'semanticResults')
          ? 'semanticResults'
          : 'entireLibrary';
    } catch {
      if (loadGeneration !== generation) return;
      error = t('knowledgeSearch', 'loadFailed');
    } finally {
      if (loadGeneration === generation) {
        busy = undefined;
        m.redraw();
      }
    }
    if (loadGeneration === generation && (draft.about ?? []).length > 0) {
      await reinterpret(attrs);
    }
  }

  async function previewPlan(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    // `busy` is shared, and the plan disclosure's summary cannot be disabled:
    // clearing it here would hide Cancel and re-enable Search mid-search.
    if (busy !== undefined || !canSearch()) return;
    const planGeneration = generation;
    const planRevision = revision;
    busy = 'planning';
    error = undefined;
    try {
      const previewed = await attrs.client.planKnowledgeSearch({
        draft: currentDraft(),
        scope: scope(attrs),
        mode,
        options: searchOptions(),
      });
      // A plan built for a query the user has already edited away would
      // describe a search they can no longer run.
      if (planGeneration !== generation || planRevision !== revision) return;
      plan = previewed;
    } catch {
      if (planGeneration !== generation || planRevision !== revision) return;
      error = t('knowledgeSearch', 'planFailed');
    } finally {
      if (planGeneration === generation) {
        busy = undefined;
        m.redraw();
      }
    }
  }

  /** Whether the canonical draft carries at least one retrievable subject. */
  function hasSubject(): boolean {
    return (draft.about ?? []).some((subject) => subject.trim().length > 0);
  }

  /** Whether a search can run: a subject, and no unusable scope selector. */
  function canSearch(): boolean {
    return hasSubject() && scopeIssues.length === 0;
  }

  /**
   * Runs the search against the *latest* interpreted query. A parse still in
   * flight is awaited first, so a search can never be built from a half-applied
   * draft, and a response that arrives after a further edit, a close or a
   * reopen is discarded rather than shown.
   */
  async function search(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    if (busy !== undefined) return;
    // A query typed only into the DSL box has not reached the canonical draft
    // until its parse lands, so an in-flight parse is a reason to wait, not a
    // reason to refuse.
    if (pendingParse === undefined && !canSearch()) return;
    const startGeneration = generation;
    const controller = new AbortController();
    busy = 'searching';
    error = undefined;
    notice = undefined;
    result = undefined;
    // A new search replaces the evidence set entirely, so any answer over the
    // previous one is dropped before the first byte of the new one arrives.
    resetAnswer();
    abortController = controller;
    m.redraw();
    if (pendingParse !== undefined) await pendingParse;
    if (startGeneration !== generation) return;
    if (controller.signal.aborted || !canSearch()) {
      if (controller.signal.aborted) notice = t('knowledgeSearch', 'cancelled');
      busy = undefined;
      abortController = undefined;
      m.redraw();
      return;
    }
    const searchRevision = revision;
    traceRequested = includeTrace;
    const requestId = crypto.randomUUID();
    try {
      const executed = await attrs.client.executeKnowledgeSearch(
        {
          requestId,
          draft: currentDraft(),
          scope: scope(attrs),
          mode,
          options: searchOptions(),
        },
        controller.signal,
      );
      if (startGeneration !== generation || searchRevision !== revision) return;
      result = executed;
      plan = executed.plan;
      capabilities = executed.capabilities;
    } catch (cause) {
      if (startGeneration !== generation || searchRevision !== revision) return;
      if (cause instanceof DOMException && cause.name === 'AbortError') {
        notice = t('knowledgeSearch', 'cancelled');
      } else {
        error = knowledgeSearchErrorMessage(cause);
      }
    } finally {
      if (startGeneration === generation) {
        busy = undefined;
        abortController = undefined;
        m.redraw();
      }
    }
  }

  function cancel(): void {
    abortController?.abort();
  }

  /** Whether an optional answer may be offered at all for what is on screen. */
  function answerAvailable(): boolean {
    return result?.capabilities.answerGeneration === true;
  }

  /** Whether the Generate control can run right now. */
  function canGenerateAnswer(): boolean {
    return (
      answerAvailable() &&
      !generatingAnswer &&
      busy === undefined &&
      selectedAnswerProfileId !== '' &&
      answerProfiles.some((profile) => profile.id === selectedAnswerProfileId)
    );
  }

  /**
   * Generates one optional answer over the evidence already on screen.
   *
   * Retrieval is never involved: the request names the exact
   * `evidenceFingerprint` the displayed result reported, and the answer-only
   * fields come from the canonical draft that produced it. A response that
   * lands after an edit, a new search, a close, a reopen or a newer generation
   * is discarded, and a host that no longer retains the evidence is reported as
   * "search again" rather than silently retrieved for.
   */
  async function generateAnswer(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    const inspected = result;
    if (inspected === undefined || !canGenerateAnswer()) return;
    const startGeneration = generation;
    const answerRevision = revision;
    const fingerprint = inspected.evidenceFingerprint;
    const profileId = selectedAnswerProfileId;
    const modelKnowledge = allowModelKnowledge;
    resetAnswer();
    const sequence = answerSequence;
    const controller = new AbortController();
    answerAbortController = controller;
    generatingAnswer = true;
    m.redraw();
    /** Whether this response still belongs to what is on screen. */
    const superseded = (): boolean =>
      sequence !== answerSequence ||
      startGeneration !== generation ||
      answerRevision !== revision ||
      result?.evidenceFingerprint !== fingerprint;
    try {
      const generated = await attrs.client.generateKnowledgeAnswer(
        {
          requestId: crypto.randomUUID(),
          workspaceId: attrs.workspaceId,
          evidenceFingerprint: fingerprint,
          profileId,
          allowModelKnowledge: modelKnowledge,
          action: draft.action ?? null,
          context: draft.context ?? null,
          constraints: [...(draft.constraints ?? [])],
          depth: draft.depth ?? null,
          output: draft.format ?? null,
        },
        controller.signal,
      );
      if (superseded()) return;
      answer = generated;
      focusAnswerOnReady = true;
    } catch (cause) {
      if (superseded()) return;
      if (cause instanceof DOMException && cause.name === 'AbortError') {
        answerNotice = t('knowledgeSearch', 'answerCancelled');
      } else if (isKnowledgeEvidenceRefreshRequired(cause)) {
        // Retrieving again here would silently spend authorization the user
        // did not ask to spend; they are told to press Search instead.
        answerError = t('knowledgeSearch', 'answerRefreshRequired');
      } else {
        answerError = t('knowledgeSearch', 'answerFailed');
      }
    } finally {
      if (sequence === answerSequence && startGeneration === generation) {
        generatingAnswer = false;
        answerAbortController = undefined;
        m.redraw();
      }
    }
  }

  function cancelAnswer(): void {
    answerAbortController?.abort();
  }

  async function openSource(
    attrs: KnowledgeSearchDialogAttrs,
    evidence: KnowledgeEvidence,
  ): Promise<void> {
    if (attrs.onOpenSource === undefined) return;
    error = undefined;
    try {
      await attrs.onOpenSource(evidence);
    } catch {
      error = t('knowledgeSearch', 'openSourceFailed');
      m.redraw();
    }
  }

  function diagnosticText(diagnostic: KnowledgeDiagnostic): string {
    const suggestion =
      diagnostic.suggestion == null
        ? ''
        : ` ${t('knowledgeSearch', 'diagnosticSuggestion', { suggestion: diagnostic.suggestion })}`;
    return `${diagnostic.message}${suggestion}`;
  }

  /** Actionable text for a DSL scope selector the composer cannot apply. */
  function scopeIssueText(issue: KnowledgeScopeIssue): string {
    const selector = scopeSelectorText(issue.selector);
    return issue.reason === 'unknownRoot'
      ? t('knowledgeSearch', 'dslScopeUnknownRoot', { selector })
      : t('knowledgeSearch', 'dslScopeUnsupported', { selector });
  }

  /** Cancels any running search and closes, invalidating in-flight responses. */
  function close(attrs: KnowledgeSearchDialogAttrs): void {
    const wasSearching = busy === 'searching';
    cancel();
    // An answer in flight belongs to the result this dialog is losing, so it is
    // stopped here rather than left running for nobody.
    resetAnswer();
    // A new generation drops every response still in flight: after a close the
    // dialog must never adopt a plan, parse or result for the query it had.
    generation += 1;
    pendingParse = undefined;
    busy = undefined;
    if (wasSearching) notice = t('knowledgeSearch', 'cancelled');
    attrs.onClose();
  }

  /** Keeps editor keystrokes local and lets Escape dismiss only the settings modal. */
  function stopSettingsTypingKeys(event: KeyboardEvent): void {
    event.stopPropagation();
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      settingsOpen = false;
    }
  }

  function evidenceSection(
    attrs: KnowledgeSearchDialogAttrs,
    row: KnowledgeEvidence,
    key: string,
  ): m.Vnode {
    const position = knowledgeProvenanceLabel(row.provenance);
    const indexedTitle = row.sectionPath.join(' / ');
    const relevanceStrength = row.finalRank <= 3 ? 'high' : row.finalRank <= 10 ? 'medium' : 'low';
    const relevanceKey =
      relevanceStrength === 'high'
        ? 'resultRelevanceHigh'
        : relevanceStrength === 'medium'
          ? 'resultRelevanceMedium'
          : 'resultRelevanceLow';
    const relevance = t('knowledgeSearch', relevanceKey);
    const openable = !row.unavailable && attrs.onOpenSource !== undefined;
    const states = [
      row.adjacent ? t('knowledgeSearch', 'adjacentEvidence') : undefined,
      row.generated ? t('knowledgeSearch', 'generatedEvidence') : undefined,
      row.stale === true ? t('knowledgeSearch', 'staleEvidence') : undefined,
      row.stale == null ? t('knowledgeSearch', 'unknownFreshness') : undefined,
      row.unavailable ? t('knowledgeSearch', 'unavailableEvidence') : undefined,
      row.duplicateSourceIds.length === 0
        ? undefined
        : t('knowledgeSearch', 'duplicateSources', { count: row.duplicateSourceIds.length }),
    ].filter((value): value is string => value !== undefined);
    return m('section.fm-knowledge-result', { key }, [
      m('.fm-knowledge-section-heading', [
        indexedTitle === '' ? undefined : m('span.fm-knowledge-section-title', indexedTitle),
        m('.fm-knowledge-section-metadata', [
          position === ''
            ? undefined
            : m(
                'button.fm-knowledge-page-link',
                {
                  type: 'button',
                  disabled: !openable,
                  'aria-label': t('knowledgeSearch', 'openSection', { section: position }),
                  onclick: () => void openSource(attrs, row),
                },
                position,
              ),
          tooltip(
            relevance,
            m('span.fm-knowledge-relevance-indicator', {
              role: 'img',
              tabindex: 0,
              'aria-label': relevance,
              'data-strength': relevanceStrength,
            }),
            { 'data-tooltip-placement': 'above' },
          ),
        ]),
      ]),
      m('.fm-knowledge-result-markdown', m.trust(safeMarkdownHtml(row.content))),
      states.length === 0
        ? undefined
        : m(
            'p.fm-knowledge-state',
            states.flatMap((state, index) => (index === 0 ? [state] : [' · ', state])),
          ),
    ]);
  }

  function evidenceDocument(
    attrs: KnowledgeSearchDialogAttrs,
    group: ReturnType<typeof groupEvidenceByDocument>[number],
    index: number,
  ): m.Vnode {
    const openable = !group.openEvidence.unavailable && attrs.onOpenSource !== undefined;
    return m(
      'li.fm-knowledge-document-item',
      { key: `document-${group.documentId}`, value: index + 1 },
      m('article.fm-knowledge-group', [
        m('.fm-knowledge-document-heading', [
          m('span.fm-knowledge-document-number', { 'aria-hidden': 'true' }, `${index + 1}.`),
          m('h4.fm-knowledge-result-heading', [
            m(
              'button.fm-knowledge-source-link',
              {
                type: 'button',
                disabled: !openable,
                'aria-label': t('knowledgeSearch', 'openSource', { title: group.title }),
                title: group.title,
                onclick: () => void openSource(attrs, group.openEvidence),
              },
              [externalLinkIcon({ size: 14 }), m('span', group.title)],
            ),
          ]),
        ]),
        m(
          '.fm-knowledge-sections',
          group.rows.map((row) =>
            evidenceSection(attrs, row, `${group.documentId}-${row.recordId}`),
          ),
        ),
      ]),
    );
  }

  /**
   * Renders the privacy-safe retrieval trace the search was asked for: the
   * bounded planned queries with their per-route candidate counts, the fused
   * row count, and the rank constant that produced the fusion. When a trace was
   * requested but the host returned none, that is said explicitly rather than
   * leaving the checkbox looking like it did nothing.
   */
  function traceView(): m.Children {
    if (result === undefined || !traceRequested) return undefined;
    const trace = result.trace;
    if (trace == null) {
      return m(
        'p.fm-knowledge-trace-body.fm-knowledge-hint',
        { role: 'status' },
        t('knowledgeSearch', 'traceUnavailable'),
      );
    }
    return m('.fm-knowledge-trace-body', [
      m('h4', t('knowledgeSearch', 'traceHeading')),
      m(
        'p',
        t('knowledgeSearch', 'traceSummary', {
          queries: trace.queries.length,
          fused: result.evidence.length,
          constant: trace.rankConstant,
        }),
      ),
      m(
        'ol.fm-knowledge-trace-queries',
        trace.queries.map((query, index) =>
          m('li', { key: `trace-${index}` }, [
            m('code', query.text),
            m(
              'small.fm-knowledge-hint',
              ` · ${t('knowledgeSearch', 'traceCandidates', {
                fullText: query.fullTextCandidates,
                semantic: query.semanticCandidates,
              })}`,
            ),
          ]),
        ),
      ),
    ]);
  }

  function resultsView(attrs: KnowledgeSearchDialogAttrs): m.Children {
    if (busy === 'searching') {
      return m('p.fm-knowledge-status', { role: 'status' }, t('knowledgeSearch', 'searching'));
    }
    if (result === undefined) {
      return m('p.fm-knowledge-status', { role: 'status' }, notice ?? t('knowledgeSearch', 'idle'));
    }
    if (result.evidence.length === 0) {
      return m('.fm-knowledge-empty', [
        m('p', { role: 'status' }, t('knowledgeSearch', 'empty')),
        m('p.fm-knowledge-hint', t('knowledgeSearch', 'emptyHint')),
      ]);
    }
    return m(
      'ol.fm-knowledge-document-list',
      groupEvidenceByDocument(result.evidence).map((group, index) =>
        evidenceDocument(attrs, group, index),
      ),
    );
  }

  /** Localised endpoint classification, shown before anything is sent. */
  function localityLabel(locality: LlmEndpointLocality): string {
    return locality === 'cloud'
      ? t('knowledgeSearch', 'answerLocalityCloud')
      : t('knowledgeSearch', 'answerLocalityLoopback');
  }

  /**
   * The rows the search displayed, keyed by the identity a citation must
   * match. A citation is only ever opened through an identity that is in here,
   * so an answer can never navigate to something the user did not inspect.
   */
  function displayedEvidence(): ReadonlyMap<string, KnowledgeEvidence> {
    return new Map((result?.evidence ?? []).map((row) => [row.recordId, row]));
  }

  /** The displayed row one citation names, if it names one at all. */
  function citedRow(
    displayed: ReadonlyMap<string, KnowledgeEvidence>,
    citation: KnowledgeAnswerCitation,
  ): KnowledgeEvidence | undefined {
    const row = displayed.get(citation.recordId);
    return row === undefined || row.sourceId !== citation.sourceId ? undefined : row;
  }

  /** The generated answer itself, plus how honest it is about its evidence. */
  function answerBody(
    attrs: KnowledgeSearchDialogAttrs,
    displayed: ReadonlyMap<string, KnowledgeEvidence>,
  ): m.Children {
    if (generatingAnswer) {
      return m(
        'p.fm-knowledge-status',
        { role: 'status' },
        t('knowledgeSearch', 'generatingAnswer'),
      );
    }
    const current = answer;
    if (current === undefined) {
      return m(
        'p.fm-knowledge-status',
        { role: 'status' },
        answerNotice ?? t('knowledgeSearch', 'answerPlaceholder'),
      );
    }
    const openable = (citation: KnowledgeAnswerCitation): boolean =>
      citedRow(displayed, citation) !== undefined &&
      !citation.unavailable &&
      attrs.onOpenSource !== undefined;
    return [
      m(
        'p.fm-knowledge-hint',
        t('knowledgeSearch', 'answerProfileUsed', {
          profile: current.profileName,
          locality: localityLabel(current.locality),
        }),
      ),
      m(
        '.fm-knowledge-answer-markdown',
        {
          onclick: (event: MouseEvent) => {
            if (!(event.target instanceof Element)) return;
            const link = event.target.closest<HTMLAnchorElement>(`a[href^="${CITATION_ANCHOR}"]`);
            if (link === null) return;
            const target = link.getAttribute('href');
            const label =
              target === null
                ? undefined
                : decodeURIComponent(target.slice(CITATION_ANCHOR.length));
            const citation = current.citations.find((item) => item.label === label);
            if (citation === undefined || !openable(citation)) return;
            event.preventDefault();
            const row = citedRow(displayed, citation);
            if (row !== undefined) void openSource(attrs, row);
          },
        },
        m.trust(
          safeMarkdownHtml(
            // Only citations that name a displayed row are linked; anything
            // else stays inert text rather than becoming clickable.
            linkKnowledgeCitations(current.text, current.citations.filter(openable)),
          ),
        ),
      ),
      current.modelKnowledgeAllowed
        ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'modelKnowledgeUsed'))
        : undefined,
      current.insufficient
        ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'answerInsufficient'))
        : undefined,
      current.staleEvidence === 0
        ? undefined
        : m(
            'p.fm-knowledge-warning',
            t('knowledgeSearch', 'answerStaleEvidence', { count: current.staleEvidence }),
          ),
      current.unavailableEvidence === 0
        ? undefined
        : m(
            'p.fm-knowledge-warning',
            t('knowledgeSearch', 'answerUnavailableEvidence', {
              count: current.unavailableEvidence,
            }),
          ),
      current.withheldUnauthorized === 0
        ? undefined
        : m(
            'p.fm-knowledge-warning',
            t('knowledgeSearch', 'answerWithheldUnauthorized', {
              count: current.withheldUnauthorized,
            }),
          ),
      current.citations.length === 0
        ? undefined
        : m('.fm-knowledge-answer-citations', [
            m('h4', t('knowledgeSearch', 'answerCitations')),
            m(
              'ul.fm-knowledge-citations',
              current.citations.map((citation) => {
                const row = citedRow(displayed, citation);
                const provenance = knowledgeProvenanceLabel(citation.provenance);
                const states = [
                  row?.title,
                  citation.sectionPath.length === 0 ? undefined : citation.sectionPath.join(' / '),
                  provenance === '' ? undefined : provenance,
                  citation.generated ? t('knowledgeSearch', 'generatedEvidence') : undefined,
                  citation.stale === true ? t('knowledgeSearch', 'staleEvidence') : undefined,
                  citation.unavailable ? t('knowledgeSearch', 'unavailableEvidence') : undefined,
                  row === undefined
                    ? t('knowledgeSearch', 'answerCitationNotDisplayed')
                    : undefined,
                ].filter((value): value is string => value !== undefined && value !== '');
                return m('li', { key: `${citation.label}-${citation.recordId}` }, [
                  m(
                    'button.fm-knowledge-source-link',
                    {
                      type: 'button',
                      disabled: !openable(citation),
                      'aria-label': t('knowledgeSearch', 'openCitation', {
                        label: citation.label,
                      }),
                      onclick: () => {
                        const row = citedRow(displayed, citation);
                        if (row !== undefined) void openSource(attrs, row);
                      },
                    },
                    citation.label,
                  ),
                  states.length === 0 ? '' : ` · ${states.join(' · ')}`,
                ]);
              }),
            ),
          ]),
    ];
  }

  /**
   * The optional answer section: strictly downstream of a completed search.
   *
   * It exists only while a successful result whose own capabilities report
   * `answerGeneration` is on screen, so a search-only host renders nothing here
   * and search stays complete without it.
   */
  function answerView(attrs: KnowledgeSearchDialogAttrs): m.Children {
    if (!answerAvailable()) return undefined;
    const displayed = displayedEvidence();
    const profile = answerProfiles.find((candidate) => candidate.id === selectedAnswerProfileId);
    return m(
      'section.fm-knowledge-answer',
      { 'aria-label': t('knowledgeSearch', 'answerRegion'), 'aria-busy': generatingAnswer },
      [
        m('h3', t('knowledgeSearch', 'answerHeading')),
        m('p.fm-knowledge-hint', t('knowledgeSearch', 'answerHint')),
        answerProfilesFailed
          ? m(
              'p.fm-knowledge-error',
              { role: 'alert' },
              t('knowledgeSearch', 'answerProfilesFailed'),
            )
          : answerProfiles.length === 0
            ? m('p.fm-knowledge-hint', { role: 'status' }, t('knowledgeSearch', 'answerNoProfiles'))
            : m('.fm-knowledge-answer-controls', [
                m('.fm-knowledge-field', [
                  m(
                    'label',
                    { for: 'fm-knowledge-answer-profile' },
                    t('knowledgeSearch', 'answerProfile'),
                  ),
                  m(
                    'select#fm-knowledge-answer-profile.browser-default',
                    {
                      name: 'knowledge-answer-profile',
                      value: selectedAnswerProfileId,
                      disabled: generatingAnswer,
                      onchange: (event: Event) => {
                        selectedAnswerProfileId = (event.currentTarget as HTMLSelectElement).value;
                        // A different endpoint is a different disclosure, so
                        // the previous answer cannot stand.
                        resetAnswer();
                      },
                    },
                    [
                      m('option', { value: '' }, t('knowledgeSearch', 'answerProfilePlaceholder')),
                      // Unkeyed on purpose: Mithril refuses a sibling list
                      // that mixes keyed and unkeyed vnodes, and the
                      // placeholder option cannot carry a profile key.
                      ...answerProfiles.map((candidate) =>
                        m(
                          'option',
                          { value: candidate.id },
                          `${candidate.name} · ${localityLabel(candidate.locality)}`,
                        ),
                      ),
                    ],
                  ),
                ]),
                m('.fm-knowledge-answer-option', [
                  m('input#fm-knowledge-model-knowledge', {
                    type: 'checkbox',
                    checked: allowModelKnowledge,
                    disabled: generatingAnswer,
                    onchange: (event: Event) => {
                      allowModelKnowledge = (event.currentTarget as HTMLInputElement).checked;
                      resetAnswer();
                    },
                  }),
                  m(
                    'label',
                    { for: 'fm-knowledge-model-knowledge' },
                    t('knowledgeSearch', 'allowModelKnowledge'),
                  ),
                ]),
              ]),
        allowModelKnowledge
          ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'modelKnowledgeNotice'))
          : undefined,
        profile === undefined
          ? undefined
          : m(
              profile.locality === 'cloud' ? 'p.fm-knowledge-warning' : 'p.fm-knowledge-hint',
              { role: 'status' },
              profile.locality === 'cloud'
                ? t('knowledgeSearch', 'answerCloudEndpoint', { profile: profile.name })
                : t('knowledgeSearch', 'answerLocalEndpoint', { profile: profile.name }),
            ),
        answerProfiles.length === 0
          ? undefined
          : m('.fm-knowledge-answer-actions', [
              m(
                FlatButton,
                {
                  type: 'button',
                  className: 'fm-knowledge-generate',
                  disabled: !canGenerateAnswer(),
                  onclick: () => void generateAnswer(attrs),
                },
                generatingAnswer
                  ? t('knowledgeSearch', 'generatingAnswer')
                  : t('knowledgeSearch', 'generateAnswer'),
              ),
              generatingAnswer
                ? m(
                    FlatButton,
                    { type: 'button', onclick: cancelAnswer },
                    t('knowledgeSearch', 'cancelAnswer'),
                  )
                : undefined,
            ]),
        answerError === undefined
          ? undefined
          : m('p.fm-knowledge-error', { role: 'alert' }, answerError),
        m(
          '.fm-knowledge-answer-body',
          {
            tabindex: '-1',
            'aria-live': 'polite',
            onupdate: ({ dom }: m.VnodeDOM) => {
              if (focusAnswerOnReady && answer !== undefined) {
                focusAnswerOnReady = false;
                (dom as HTMLElement).focus();
              }
            },
          },
          answerBody(attrs, displayed),
        ),
      ],
    );
  }

  return {
    oninit: ({ attrs }) => {
      wasOpen = attrs.open;
      if (attrs.open) void load(attrs);
    },
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) void load(attrs);
      wasOpen = attrs.open;
      // The modal keeps its content mounted while closed, so the subject
      // field's own `oncreate` only ever runs once, long before the dialog is
      // opened. Focus is therefore taken on the closed -> open transition.
      if (attrs.open && focusSubjectOnOpen && busy !== 'loading') {
        const subject = document.querySelector<HTMLTextAreaElement>('#fm-knowledge-subjects');
        if (subject !== null) {
          focusSubjectOnOpen = false;
          subject.focus();
          subject.setSelectionRange(subject.value.length, subject.value.length);
        }
      }
    },
    onremove: () => abortController?.abort(),
    view: ({ attrs }) => {
      const offline = attrs.client.connection.get() === 'closed';
      const searchable = canSearch();
      const route = result?.route;
      if (!attrs.open) return undefined;
      return m(
        'section#fm-knowledge-search-pane.fm-knowledge-search',
        { 'aria-label': t('knowledgeSearch', 'title') },
        [
          m('.fm-knowledge-composer', [
            m('.fm-knowledge-search-toolbar', [
              filterIcon({ className: 'fm-knowledge-search-icon', size: 14 }),
              m('textarea#fm-knowledge-subjects', {
                name: 'knowledge-subjects',
                rows: 1,
                value: subjectsText,
                disabled: busy === 'loading' || busy === 'searching',
                autocomplete: 'off',
                'aria-label': t('knowledgeSearch', 'subjects'),
                placeholder: t('knowledgeSearch', 'subjectsPlaceholder'),
                onupdate: ({ dom }: m.VnodeDOM) => {
                  if (!focusSubjectOnOpen || busy === 'loading') return;
                  focusSubjectOnOpen = false;
                  const input = dom as HTMLTextAreaElement;
                  input.focus();
                  input.setSelectionRange(input.value.length, input.value.length);
                },
                oninput: (event: InputEvent) => {
                  subjectsText = (event.currentTarget as HTMLTextAreaElement).value;
                  draft = { ...draft, about: splitSubjects(subjectsText) };
                  edited();
                  void reinterpret(attrs);
                },
                onkeydown: (event: KeyboardEvent) => {
                  event.stopPropagation();
                  if (event.key === 'Escape' && !event.isComposing) {
                    event.preventDefault();
                    close(attrs);
                    return;
                  }
                  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
                    event.preventDefault();
                    void search(attrs);
                  }
                },
              }),
              busy === 'searching'
                ? m('span.fm-knowledge-search-spinner', {
                    role: 'status',
                    'aria-label': t('knowledgeSearch', 'searching'),
                  })
                : undefined,
              tooltip(
                t('knowledgeSearch', 'search'),
                m(
                  IconButton,
                  {
                    type: 'button',
                    className: 'fm-knowledge-search-submit',
                    'aria-label': t('knowledgeSearch', 'search'),
                    disabled: busy !== undefined || !canSearch(),
                    onclick: () => void search(attrs),
                  },
                  cornerDownLeftIcon({ size: 16 }),
                ),
              ),
              tooltip(
                t('knowledgeSearch', 'settings'),
                m(
                  IconButton,
                  {
                    type: 'button',
                    className: 'fm-knowledge-settings-trigger',
                    'aria-label': t('knowledgeSearch', 'settings'),
                    onclick: () => {
                      settingsOpen = true;
                    },
                  },
                  settingsIcon({ size: 16 }),
                ),
              ),
            ]),
            m(ModalPanel, {
              className: 'fm-dense-modal fm-knowledge-settings-modal',
              title: t('knowledgeSearch', 'settings'),
              isOpen: settingsOpen,
              closeOnEsc: true,
              onToggle: (open: boolean) => {
                settingsOpen = open;
              },
              description: m('.fm-knowledge-settings', [
                m('fieldset.fm-knowledge-needs', [
                  m('legend', t('knowledgeSearch', 'needs')),
                  NEEDS.map((need) =>
                    m('label', { key: need }, [
                      m('input', {
                        type: 'checkbox',
                        checked: (draft.needs ?? []).includes(need),
                        disabled: busy === 'searching',
                        onchange: (event: Event) => {
                          const selected = new Set(draft.needs ?? []);
                          if ((event.currentTarget as HTMLInputElement).checked) selected.add(need);
                          else selected.delete(need);
                          const needs = NEEDS.filter((value) => selected.has(value));
                          draft = { ...draft, needs };
                          persistPreferredNeeds(needs);
                          edited();
                          void reinterpret(attrs);
                        },
                      }),
                      m('span', needLabel(need)),
                    ]),
                  ),
                ]),
                m('details.fm-knowledge-advanced', [
                  m('summary', t('knowledgeSearch', 'showAdvanced')),
                  m('.fm-knowledge-advanced-body', [
                    m('.fm-knowledge-row', [
                      m('label.fm-knowledge-field', [
                        m('span', t('knowledgeSearch', 'scope')),
                        m(
                          'select#fm-knowledge-scope.browser-default',
                          {
                            name: 'knowledge-scope',
                            value: scopeKind,
                            disabled: busy === 'searching',
                            onchange: (event: Event) => {
                              scopeKind = (event.currentTarget as HTMLSelectElement)
                                .value as KnowledgeScopeKind;
                              edited();
                              void reinterpret(attrs);
                            },
                          },
                          SCOPE_KINDS.map((kind) =>
                            m(
                              'option',
                              { key: kind, value: kind, disabled: !scopeAvailable(attrs, kind) },
                              scopeKindLabel(kind),
                            ),
                          ),
                        ),
                      ]),
                      m('fieldset.fm-knowledge-modes', [
                        m('legend', t('knowledgeSearch', 'mode')),
                        MODES.map((candidate) =>
                          m(
                            'button',
                            {
                              key: candidate,
                              type: 'button',
                              class:
                                mode === candidate
                                  ? 'fm-knowledge-mode is-selected'
                                  : 'fm-knowledge-mode',
                              'aria-pressed': mode === candidate ? 'true' : 'false',
                              disabled: !modeAvailable(candidate) || busy === 'searching',
                              title: modeAvailable(candidate)
                                ? undefined
                                : t('knowledgeSearch', 'modeSemanticUnavailable'),
                              onclick: () => {
                                mode = candidate;
                                edited();
                              },
                            },
                            modeLabel(candidate),
                          ),
                        ),
                      ]),
                    ]),
                    capabilities?.semantic === false
                      ? m(
                          'p.fm-knowledge-hint',
                          { role: 'status' },
                          mode === 'hybrid'
                            ? t('knowledgeSearch', 'hybridFallsBackToFullText')
                            : t('knowledgeSearch', 'modeSemanticUnavailable'),
                        )
                      : undefined,
                    capabilities?.fullText === false
                      ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'fullTextUnavailable'))
                      : undefined,
                    scopeKind === 'enrolledRoots'
                      ? m('fieldset.fm-knowledge-roots', [
                          m('legend', t('knowledgeSearch', 'roots')),
                          roots.length === 0
                            ? m('p.fm-knowledge-hint', t('knowledgeSearch', 'rootsEmpty'))
                            : roots.map((root) =>
                                m('label', { key: root.rootId }, [
                                  m('input', {
                                    type: 'checkbox',
                                    checked: selectedRootIds.has(root.rootId),
                                    disabled: busy === 'searching',
                                    onchange: (event: Event) => {
                                      if ((event.currentTarget as HTMLInputElement).checked)
                                        selectedRootIds.add(root.rootId);
                                      else selectedRootIds.delete(root.rootId);
                                      edited();
                                      void reinterpret(attrs);
                                    },
                                  }),
                                  m(
                                    'span',
                                    root.available
                                      ? root.label
                                      : `${root.label} · ${t('knowledgeSearch', 'rootUnavailable')}`,
                                  ),
                                ]),
                              ),
                        ])
                      : undefined,
                    m('details.fm-knowledge-dsl', [
                      m('summary', t('knowledgeSearch', 'dsl')),
                      m('textarea#fm-knowledge-dsl-input', {
                        name: 'knowledge-dsl',
                        rows: 4,
                        value: dslText,
                        disabled: busy === 'searching',
                        autocomplete: 'off',
                        'aria-describedby': 'fm-knowledge-dsl-hint',
                        oninput: (event: InputEvent) => {
                          dslText = (event.currentTarget as HTMLTextAreaElement).value;
                          edited();
                          void reinterpret(attrs, dslText);
                        },
                        onkeydown: stopSettingsTypingKeys,
                      }),
                      m(
                        'small#fm-knowledge-dsl-hint.fm-knowledge-hint',
                        t('knowledgeSearch', 'dslHint'),
                      ),
                      scopeIssues.length === 0
                        ? undefined
                        : m('.fm-knowledge-scope-issues', [
                            m('h4', t('knowledgeSearch', 'dslScopeIssues')),
                            m(
                              'ul',
                              scopeIssues.map((issue, index) =>
                                m(
                                  'li.fm-knowledge-error',
                                  { key: `scope-issue-${index}`, role: 'alert' },
                                  scopeIssueText(issue),
                                ),
                              ),
                            ),
                          ]),
                      interpretation?.diagnostics.length
                        ? m(
                            'ul.fm-knowledge-diagnostics',
                            interpretation.diagnostics.map((diagnostic, index) =>
                              m(
                                'li',
                                {
                                  key: `diagnostic-${index}`,
                                  class:
                                    diagnostic.severity === 'error'
                                      ? 'fm-knowledge-error'
                                      : 'fm-knowledge-warning',
                                  role: diagnostic.severity === 'error' ? 'alert' : undefined,
                                },
                                diagnosticText(diagnostic),
                              ),
                            ),
                          )
                        : undefined,
                      interpretation?.excludedFromRetrieval.length
                        ? m('.fm-knowledge-excluded', [
                            m('h4', t('knowledgeSearch', 'excludedFromRetrieval')),
                            m(
                              'ul',
                              interpretation.excludedFromRetrieval.map((field, index) =>
                                m(
                                  'li',
                                  { key: `excluded-${index}` },
                                  t('knowledgeSearch', 'excludedField', {
                                    field: field.field,
                                    value: field.value,
                                  }),
                                ),
                              ),
                            ),
                          ])
                        : undefined,
                    ]),
                    m(
                      'details.fm-knowledge-plan',
                      {
                        ontoggle: (event: Event) => {
                          if (
                            (event.currentTarget as HTMLDetailsElement).open &&
                            plan === undefined
                          ) {
                            void previewPlan(attrs);
                          }
                        },
                      },
                      [
                        m('summary', t('knowledgeSearch', 'plan')),
                        m('label.fm-knowledge-trace', [
                          m('input', {
                            type: 'checkbox',
                            checked: includeTrace,
                            disabled: busy === 'searching',
                            onchange: (event: Event) => {
                              includeTrace = (event.currentTarget as HTMLInputElement).checked;
                            },
                          }),
                          m('span', t('knowledgeSearch', 'trace')),
                        ]),
                        traceView(),
                        plan === undefined
                          ? m(
                              FlatButton,
                              {
                                type: 'button',
                                disabled: !searchable || busy !== undefined,
                                onclick: () => void previewPlan(attrs),
                              },
                              t('knowledgeSearch', 'plan'),
                            )
                          : m('.fm-knowledge-plan-body', [
                              m(
                                'p',
                                t('knowledgeSearch', 'planVersion', { version: plan.version }),
                              ),
                              m('p', t('knowledgeSearch', 'planScope', { scope: plan.scopeLabel })),
                              m(
                                'p',
                                t('knowledgeSearch', 'planSources', {
                                  count: plan.authorizedSources,
                                }),
                              ),
                              m(
                                'p',
                                t('knowledgeSearch', 'planSearches', {
                                  count: plan.searches.length,
                                }),
                              ),
                              plan.omittedSearches === 0
                                ? undefined
                                : m(
                                    'p.fm-knowledge-warning',
                                    t('knowledgeSearch', 'planOmitted', {
                                      count: plan.omittedSearches,
                                    }),
                                  ),
                              m(
                                'ol.fm-knowledge-planned-searches',
                                plan.searches.map((planned, index) =>
                                  m('li', { key: `planned-${index}` }, [
                                    m('code', planned.text),
                                    m(
                                      'small.fm-knowledge-hint',
                                      ` · ${priorityLabel(planned.priority)}`,
                                    ),
                                    m(
                                      'ul',
                                      planned.reasons.map((reason, reasonIndex) =>
                                        m(
                                          'li',
                                          { key: `planned-${index}-reason-${reasonIndex}` },
                                          reasonLabel(reason),
                                        ),
                                      ),
                                    ),
                                  ]),
                                ),
                              ),
                              plan.excludedFromRetrieval.length === 0
                                ? undefined
                                : m('.fm-knowledge-excluded', [
                                    m('h4', t('knowledgeSearch', 'excludedFromRetrieval')),
                                    m(
                                      'ul',
                                      plan.excludedFromRetrieval.map((field, index) =>
                                        m(
                                          'li',
                                          { key: `plan-excluded-${index}` },
                                          t('knowledgeSearch', 'excludedField', {
                                            field: field.field,
                                            value: field.value,
                                          }),
                                        ),
                                      ),
                                    ),
                                  ]),
                            ]),
                      ],
                    ),
                    result === undefined
                      ? undefined
                      : m('details.fm-knowledge-result-details', [
                          m('summary', t('knowledgeSearch', 'searchDetails')),
                          route === undefined
                            ? undefined
                            : m(
                                route.fallbackReason == null ? 'p' : 'p.fm-knowledge-warning',
                                route.fallbackReason == null
                                  ? t('knowledgeSearch', 'routeApplied', {
                                      route: modeLabel(route.applied),
                                    })
                                  : t('knowledgeSearch', 'routeFallback', {
                                      requested: modeLabel(route.requested),
                                      applied: modeLabel(route.applied),
                                      reason: fallbackReasonLabel(route.fallbackReason),
                                    }),
                              ),
                          m(
                            'p',
                            result.coverage.indexed == null
                              ? t('knowledgeSearch', 'coverageUnknownIndexed', {
                                  eligible: result.coverage.eligible,
                                  fingerprinted: result.coverage.fingerprinted,
                                  unavailable: result.coverage.unavailable,
                                })
                              : t('knowledgeSearch', 'coverage', {
                                  indexed: result.coverage.indexed,
                                  eligible: result.coverage.eligible,
                                  fingerprinted: result.coverage.fingerprinted,
                                  unavailable: result.coverage.unavailable,
                                }),
                          ),
                          result.coverage.partial
                            ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'coveragePartial'))
                            : undefined,
                          result.coverage.scopeIsExact
                            ? undefined
                            : m('p.fm-knowledge-warning', t('knowledgeSearch', 'coverageInexact')),
                          result.withheldUnauthorized === 0
                            ? undefined
                            : m(
                                'p.fm-knowledge-warning',
                                t('knowledgeSearch', 'withheldUnauthorized', {
                                  count: result.withheldUnauthorized,
                                }),
                              ),
                        ]),
                  ]),
                ]),
              ]),
            }),
          ]),
          error === undefined ? undefined : m('p.fm-knowledge-error', { role: 'alert' }, error),
          offline
            ? m('p.fm-knowledge-warning', { role: 'status' }, t('knowledgeSearch', 'offline'))
            : undefined,
          busy === 'loading'
            ? m('p.fm-knowledge-status', { role: 'status' }, t('knowledgeSearch', 'loading'))
            : undefined,
          m(
            'section.fm-knowledge-results-section',
            {
              'aria-label': t('knowledgeSearch', 'resultsRegion'),
              'aria-busy': busy === 'searching' ? 'true' : 'false',
            },
            [
              m(
                '.fm-knowledge-results-heading',
                { role: result === undefined ? undefined : 'status' },
                m('h3', [
                  t('knowledgeSearch', 'results'),
                  result === undefined
                    ? undefined
                    : `: ${t('knowledgeSearch', 'resultsSummary', {
                        documents: groupEvidenceByDocument(result.evidence).length,
                        sections: result.evidence.length,
                      })}`,
                ]),
              ),
              m('.fm-knowledge-results-body', { 'aria-live': 'polite' }, [
                resultsView(attrs),
                answerView(attrs),
              ]),
            ],
          ),
        ],
      );
    },
  };
};

/** Compatibility export for task 0206 callers while the pane replaces the former modal. */
export const KnowledgeSearchDialog = KnowledgeSearchPane;
