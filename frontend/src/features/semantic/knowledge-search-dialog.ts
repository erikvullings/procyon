import m, { type FactoryComponent } from 'mithril';
import { FlatButton, IconButton, ModalPanel } from 'mithril-materialized';
import { technicalTextInputAttrs } from '../../a11y/technical-text-input';
import type { ConnectionClient } from '../../api/client/connection-client.ts';
import type { SemanticClient } from '../../api/client/semantic-client.ts';
import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import {
  checkIcon,
  chevronDownIcon,
  chevronUpIcon,
  closeIcon,
  copyIcon,
  cornerDownLeftIcon,
  dotsIcon,
  filterIcon,
  sparklesIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  KnowledgeAnswer,
  KnowledgeCapabilities,
  KnowledgeDiagnostic,
  KnowledgeEvidence,
  KnowledgeNeed,
  KnowledgeQueryDraft,
  KnowledgeQueryIntent,
  KnowledgeQueryInterpretation,
  KnowledgeRetrievalMode,
  KnowledgeRoot,
  KnowledgeScope,
  KnowledgeScopeKind,
  KnowledgeScopeSelector,
  KnowledgeSearchOptions,
  KnowledgeSearchPlan,
  KnowledgeSearchResult,
  LlmProfile,
  Location,
} from '../../models';
import { defaultKnowledgeSearchOptions } from '../../models';
import { lastPathSegment } from '../navigation/navigation';
import { copyText } from '../preview/clipboard';
import { SemanticFolderEnrolmentPrompt } from '../settings/semantic-library-management';
import { pathFromUri } from '../workspace/workspace-layout';
import { generationErrorText } from './generation-error';
import { numberKnowledgeAnswer } from './knowledge-answer-references';
import { knowledgeAnswerBody, knowledgeAskBar } from './knowledge-answer-view';
import {
  evidenceDocument,
  type KnowledgeEvidenceContext,
  knowledgeTraceView,
} from './knowledge-evidence-list';
import { groupEvidenceByDocument } from './knowledge-evidence-view';
import {
  intentLabel,
  isKnowledgeEvidenceRefreshRequired,
  knowledgeSearchErrorMessage,
  modeLabel,
  needLabel,
  scopeKindLabel,
} from './knowledge-labels';
import {
  INTENTS,
  loadPreferredNeeds,
  MODES,
  NEEDS,
  persistPreferredNeeds,
  SCOPE_KINDS,
} from './knowledge-options';
import { knowledgePlanBody, knowledgeResultDetails } from './knowledge-plan-view';
import {
  classifyKnowledgeScopeSelectors,
  emptyDraft,
  joinSubjects,
  joinValues,
  type KnowledgeScopeIssue,
  quoteValue,
  scopeSelectorText,
  splitSubjects,
} from './knowledge-scope';
import {
  blocksGeneration,
  checkProfileModel,
  type ProfileModelStatus,
  profileModelNotice,
} from './profile-model-check';

/** Everything the shell knows about the default scope when the dialog opens. */
export interface KnowledgeSearchDialogAttrs {
  readonly open: boolean;
  readonly client: ConnectionClient & SemanticClient & WorkspaceClient;
  readonly workspaceId: string;
  /** Active directory, offered as the default scope when it is indexed. */
  readonly currentFolder: Location | undefined;
  /** Opaque source ids of the active semantic result set, if any. */
  readonly semanticSourceIds: readonly string[];
  /** Initial subject text, e.g. the active quick filter or semantic query. */
  readonly initialSubject?: string | undefined;
  /** Changes when Settings may have activated or removed a generation profile. */
  readonly profileRevision?: number;
  readonly onClose: () => void;
  readonly onOpenSource?: (evidence: KnowledgeEvidence) => void | Promise<void>;
  readonly isSourceOpen?: (evidence: KnowledgeEvidence) => boolean;
  readonly onConfigureAnswers?: () => void;
}

export {
  type KnowledgeAnswerReference,
  linkKnowledgeReferences,
  type NumberedKnowledgeAnswer,
  numberKnowledgeAnswer,
} from './knowledge-answer-references';
/** Canonical need order, matching the backend enum declaration order. */
export {
  groupEvidenceByDocument,
  knowledgeProvenanceLabel,
} from './knowledge-evidence-view';
export {
  isKnowledgeEvidenceRefreshRequired,
  knowledgeSearchErrorMessage,
} from './knowledge-labels';
export {
  classifyKnowledgeScopeSelectors,
  type KnowledgeScopeIssue,
} from './knowledge-scope';

export const KnowledgeSearchPane: FactoryComponent<KnowledgeSearchDialogAttrs> = () => {
  let wasOpen = false;
  let currentAttrs: KnowledgeSearchDialogAttrs;
  let rootRefreshTimer: ReturnType<typeof setInterval> | undefined;
  let rootRefreshInFlight = false;
  let rootRefreshUnsubscribe: (() => void) | undefined;
  let removed = false;
  let busy: 'loading' | 'planning' | 'searching' | undefined;
  let error: string | undefined;
  let rootRefreshError: string | undefined;
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
  let intent: KnowledgeQueryIntent = 'search';
  let scopeKind: KnowledgeScopeKind = 'enrolledRoots';
  let selectedRootIds = new Set<string>();
  let dslText = '';
  let includeTrace = false;
  let currentFolderIndexed = false;
  let openingFolder: Location | undefined;
  let pinnedFolder: Location | undefined;
  let openedWorkspaceId: string | undefined;
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
  /** Whether each profile's model exists on its AI service; checked whenever profiles load. */
  const modelStatuses = new Map<string, ProfileModelStatus>();
  let answerProfilesFailed = false;
  let loadedProfileRevision = 0;
  let answerCopied = false;
  let answerCopyError: string | undefined;
  let answerCopyResetTimer: number | undefined;
  /** Grounded-only by default; the opt-in is explicit and labelled. */
  let allowModelKnowledge = false;
  let answer: KnowledgeAnswer | undefined;
  let answerError: string | undefined;
  let answerNotice: string | undefined;
  let generatingAnswer = false;
  let answerAbortController: AbortController | undefined;
  /** Bumped per generation, so only the newest response owns the state. */
  let answerSequence = 0;
  /** Bumped on every open/close, so responses from a previous life are dropped. */
  let generation = 0;
  /** Bumped on every edit, so a response for an older draft is never applied. */
  let revision = 0;
  /** In-flight interpretation the Enter handler must await before searching. */
  let pendingParse: Promise<void> | undefined;
  /** Set when the dialog must take focus after its next render. */
  let focusSubjectOnOpen = false;
  // Enter pressed while the pane is still loading; the search runs once it is ready.
  let searchAfterLoad = false;
  let settingsOpen = false;
  let enrolmentOpen = false;
  let answerCollapsed = false;
  let questionText = '';

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
    questionText = '';
  }

  function scope(attrs: KnowledgeSearchDialogAttrs): KnowledgeScope {
    return {
      workspaceId: attrs.workspaceId,
      kind: scopeKind,
      folder: scopeKind === 'currentFolder' ? (pinnedFolder ?? null) : null,
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
        return openingFolder !== undefined && currentFolderIndexed;
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
    if (answerCopyResetTimer !== undefined) clearTimeout(answerCopyResetTimer);
    answerCopyResetTimer = undefined;
    answerCopied = false;
    answerCopyError = undefined;
    answerAbortController?.abort();
    answerAbortController = undefined;
    answerSequence += 1;
    generatingAnswer = false;
    answer = undefined;
    answerError = undefined;
    answerNotice = undefined;
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
      pinnedFolder = undefined;
      selectedRootIds = new Set(classified.rootIds);
      return;
    }
    if (classified.wholeLibrary) {
      scopeKind = 'enrolledRoots';
      pinnedFolder = undefined;
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
    const profileRevision = attrs.profileRevision ?? 0;
    pendingParse = undefined;
    busy = 'loading';
    error = undefined;
    rootRefreshError = undefined;
    notice = undefined;
    resetResults();
    interpretation = undefined;
    questionText = '';
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
    openingFolder = attrs.currentFolder;
    pinnedFolder = undefined;
    openedWorkspaceId = attrs.workspaceId;
    includeTrace = false;
    capabilities = undefined;
    roots = [];
    selectedRootIds = new Set();
    mode = 'hybrid';
    intent = 'search';
    scopeKind = 'enrolledRoots';
    focusSubjectOnOpen = true;
    searchAfterLoad = false;
    answerProfiles = [];
    answerProfilesFailed = false;
    loadedProfileRevision = profileRevision;
    allowModelKnowledge = false;
    settingsOpen = false;
    enrolmentOpen = false;
    answerCollapsed = false;
    try {
      // Every lookup runs concurrently: each one is a separate host round-trip
      // and the dialog stays in its loading state until all have settled.
      const capabilitiesRequest = attrs.client.getKnowledgeCapabilities();
      // Generation profiles are only fetched when the host actually offers
      // answers, so a search-only host is never asked about a capability it
      // does not have, and a failure here never blocks search (task 0207).
      const profilesRequest = capabilitiesRequest.then((reported) =>
        reported.answerGeneration
          ? attrs.client.listLlmProfiles().then(
              (profiles) => ({ profiles, failed: false }),
              () => ({ profiles: [], failed: true }),
            )
          : { profiles: [], failed: false },
      );
      const currentFolder = openingFolder;
      const folderIndexedRequest =
        currentFolder === undefined
          ? Promise.resolve(false)
          : attrs.client
              .getSemanticFolderStatus({ workspaceId: attrs.workspaceId, location: currentFolder })
              .then(
                (folder) =>
                  (folder.consent === 'includedHere' || folder.consent === 'inheritedFromParent') &&
                  folder.workspaceReferenced,
                () => false,
              );
      const [reportedCapabilities, availableRoots, profileResult, folderIndexed] =
        await Promise.all([
          capabilitiesRequest,
          attrs.client.listKnowledgeRoots({ workspaceId: attrs.workspaceId }),
          profilesRequest,
          folderIndexedRequest,
        ]);
      if (loadGeneration !== generation) return;
      if (loadedProfileRevision === profileRevision) {
        capabilities = reportedCapabilities;
        answerProfiles = profileResult.profiles;
        answerProfilesFailed = profileResult.failed;
        void verifyAnswerProfiles(attrs, loadGeneration);
      }
      roots = availableRoots;
      selectedRootIds = new Set(availableRoots.map((root) => root.rootId));
      mode = reportedCapabilities.semantic
        ? 'hybrid'
        : reportedCapabilities.fullText
          ? 'hybrid'
          : 'fullText';
      if (currentFolder !== undefined) currentFolderIndexed = folderIndexed;
      scopeKind = 'enrolledRoots';
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
    if (loadGeneration === generation && searchAfterLoad) {
      searchAfterLoad = false;
      await search(attrs);
    }
  }

  async function refreshRoots(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    if (removed || !attrs.open || rootRefreshInFlight || busy === 'loading') return;
    rootRefreshInFlight = true;
    const openGeneration = generation;
    try {
      const availableRoots = await attrs.client.listKnowledgeRoots({
        workspaceId: attrs.workspaceId,
      });
      if (removed || openGeneration !== generation || !attrs.open) return;
      roots = availableRoots;
      rootRefreshError = undefined;
      m.redraw();
    } catch {
      if (!removed && openGeneration === generation && attrs.open) {
        rootRefreshError = t('knowledgeSearch', 'loadFailed');
        m.redraw();
      }
    } finally {
      rootRefreshInFlight = false;
    }
  }

  async function previewPlan(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    // `busy` is shared, and the plan disclosure's summary cannot be disabled:
    // clearing it here would hide Cancel and re-enable Search mid-search.
    if (busy !== undefined || !canSearch(attrs)) return;
    const planGeneration = generation;
    const planRevision = revision;
    busy = 'planning';
    error = undefined;
    try {
      const previewed = await attrs.client.planKnowledgeSearch({
        draft: currentDraft(),
        scope: scope(attrs),
        mode,
        intent,
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

  /** Whether an authorized source can be queried while full reconciliation continues. */
  function hasSearchableSources(attrs: KnowledgeSearchDialogAttrs): boolean {
    const indexedRoots = roots.filter(
      (root) =>
        root.available && (root.confirmedCompletedSources > 0 || root.unverifiedLegacySources > 0),
    );
    if (scopeKind === 'enrolledRoots') {
      return indexedRoots.some((root) => selectedRootIds.has(root.rootId));
    }
    return attrs.semanticSourceIds.length > 0 || indexedRoots.length > 0;
  }

  /** Whether a search can run: searchable sources, a subject, and no unusable scope selector. */
  function canSearch(attrs: KnowledgeSearchDialogAttrs): boolean {
    return hasSearchableSources(attrs) && hasSubject() && scopeIssues.length === 0;
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
    if (pendingParse === undefined && !canSearch(attrs)) return;
    const startGeneration = generation;
    const controller = new AbortController();
    busy = 'searching';
    error = undefined;
    notice = undefined;
    result = undefined;
    answerCollapsed = false;
    questionText = '';
    // A new search replaces the evidence set entirely, so any answer over the
    // previous one is dropped before the first byte of the new one arrives.
    resetAnswer();
    abortController = controller;
    m.redraw();
    if (pendingParse !== undefined) await pendingParse;
    if (startGeneration !== generation) return;
    if (controller.signal.aborted || !canSearch(attrs)) {
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
          intent,
          options: searchOptions(),
        },
        controller.signal,
      );
      if (startGeneration !== generation || searchRevision !== revision) return;
      result = executed;
      plan = executed.plan;
      capabilities = executed.capabilities;
      if (
        intent === 'questionAnswering' &&
        draft.about?.length === 1 &&
        questionText.trim().length === 0
      ) {
        questionText = draft.about[0] ?? '';
      }
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

  /**
   * Whether an LLM can answer at all: the host supports generation and at
   * least one generation profile is configured. Without one the ask bar is
   * not shown and search stays complete on its own.
   */
  function answerAvailable(): boolean {
    return (
      capabilities?.answerGeneration === true && !answerProfilesFailed && answerProfiles.length > 0
    );
  }

  async function refreshAnswerProfiles(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    const requestedRevision = attrs.profileRevision ?? 0;
    const requestedGeneration = generation;
    try {
      const reported = await attrs.client.getKnowledgeCapabilities();
      const profiles = reported.answerGeneration ? await attrs.client.listLlmProfiles() : [];
      if (generation !== requestedGeneration || loadedProfileRevision !== requestedRevision) return;
      capabilities = reported;
      answerProfiles = profiles;
      answerProfilesFailed = false;
      void verifyAnswerProfiles(attrs, requestedGeneration);
    } catch {
      if (generation !== requestedGeneration || loadedProfileRevision !== requestedRevision) return;
      answerProfiles = [];
      answerProfilesFailed = true;
    }
    m.redraw();
  }

  /**
   * Checks that every generation profile's model exists on its AI service, so a missing model is
   * reported up front instead of as a failed answer.
   */
  async function verifyAnswerProfiles(
    attrs: KnowledgeSearchDialogAttrs,
    forGeneration: number,
  ): Promise<void> {
    const checked = await Promise.all(
      answerProfiles.map(
        async (profile) => [profile.id, await checkProfileModel(attrs.client, profile)] as const,
      ),
    );
    if (generation !== forGeneration) return;
    modelStatuses.clear();
    for (const [id, status] of checked) modelStatuses.set(id, status);
    m.redraw();
  }

  /** Whether the Generate control can run right now. */
  function canGenerateAnswer(attrs: KnowledgeSearchDialogAttrs): boolean {
    return (
      answerAvailable() &&
      questionText.trim().length > 0 &&
      new TextEncoder().encode(questionText).length <= 8 * 1024 &&
      !generatingAnswer &&
      busy === undefined &&
      (result !== undefined || canSearchForQuestion(attrs)) &&
      answerProfile() !== undefined &&
      !blocksGeneration(modelStatuses.get(answerProfile()?.id ?? ''))
    );
  }

  /**
   * The default generation profile: hosts list the profile activated in
   * Settings first, and activation is where cloud consent is given.
   */
  function answerProfile(): LlmProfile | undefined {
    return answerProfiles[0];
  }

  /** Whether asking without inspected results can first search for the question. */
  function canSearchForQuestion(attrs: KnowledgeSearchDialogAttrs): boolean {
    return hasSearchableSources(attrs) && scopeIssues.length === 0;
  }

  /**
   * Asks the question. With results on screen it answers over exactly that
   * evidence; without them it first searches, as regular RAG would, using the
   * subjects already entered or else the question itself as the subject.
   */
  async function ask(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    if (!canGenerateAnswer(attrs)) return;
    if (result === undefined) {
      const asked = questionText;
      intent = 'questionAnswering';
      edited();
      if (!hasSubject()) {
        draft = { ...draft, about: [asked.trim()] };
        subjectsText = joinSubjects(draft.about ?? []);
        void reinterpret(attrs);
      }
      // search() clears the question synchronously; keep it on screen.
      const searching = search(attrs);
      questionText = asked;
      await searching;
      if (result === undefined) return;
    }
    await generateAnswer(attrs);
  }

  /**
   * Generates one optional answer over the evidence already on screen.
   *
   * Retrieval is never involved: the request names the exact
   * `evidenceFingerprint` the displayed result reported, and the answer-only
   * fields come from the canonical draft that produced it, plus a separate
   * question that is never fed to search. A response that
   * lands after an edit, a new search, a close, a reopen or a newer generation
   * is discarded, and a host that no longer retains the evidence is reported as
   * "search again" rather than silently retrieved for.
   */
  async function generateAnswer(attrs: KnowledgeSearchDialogAttrs): Promise<void> {
    const inspected = result;
    if (inspected === undefined || !canGenerateAnswer(attrs)) return;
    const startGeneration = generation;
    const answerRevision = revision;
    const fingerprint = inspected.evidenceFingerprint;
    const profileId = answerProfile()?.id ?? '';
    const modelKnowledge = allowModelKnowledge;
    resetAnswer();
    answerCollapsed = false;
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
          question: questionText.trim(),
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
    } catch (cause) {
      if (superseded()) return;
      if (cause instanceof DOMException && cause.name === 'AbortError') {
        answerNotice = t('knowledgeSearch', 'answerCancelled');
      } else if (isKnowledgeEvidenceRefreshRequired(cause)) {
        // Retrieving again here would silently spend authorization the user
        // did not ask to spend; they are told to press Search instead.
        answerError = t('knowledgeSearch', 'answerRefreshRequired');
      } else {
        answerError = generationErrorText(cause, t('knowledgeSearch', 'answerFailed'));
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
      m.redraw();
    } catch {
      error = t('knowledgeSearch', 'openSourceFailed');
      m.redraw();
    }
  }

  async function openExternalEvidence(
    attrs: KnowledgeSearchDialogAttrs,
    url: string,
  ): Promise<void> {
    error = undefined;
    try {
      await attrs.client.openExternalUrl(url);
    } catch {
      error = t('knowledgeSearch', 'openExternalFailed');
      m.redraw();
    }
  }

  function evidenceContext(attrs: KnowledgeSearchDialogAttrs): KnowledgeEvidenceContext {
    return {
      client: attrs.client,
      workspaceId: attrs.workspaceId,
      canOpen: attrs.onOpenSource !== undefined,
      isSourceOpen: (row) => attrs.isSourceOpen?.(row) === true,
      openSource: (row) => openSource(attrs, row),
      openExternal: (url) => openExternalEvidence(attrs, url),
    };
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

  function updateNeed(
    attrs: KnowledgeSearchDialogAttrs,
    need: KnowledgeNeed,
    checked: boolean,
  ): void {
    const selected = new Set(draft.needs ?? []);
    if (checked) selected.add(need);
    else selected.delete(need);
    const needs = NEEDS.filter((value) => selected.has(value));
    draft = { ...draft, needs };
    persistPreferredNeeds(needs);
    edited();
    void reinterpret(attrs);
  }

  /** Keeps editor keystrokes local and lets Escape dismiss only the settings modal. */
  function stopSettingsTypingKeys(event: KeyboardEvent): void {
    event.stopPropagation();
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      settingsOpen = false;
    }
  }

  function resultsView(attrs: KnowledgeSearchDialogAttrs): m.Children {
    if (busy === 'searching') {
      return m('p.fm-knowledge-status', { role: 'status' }, t('knowledgeSearch', 'searching'));
    }
    if (result === undefined) {
      const pendingRoots = roots.filter(
        (root) =>
          root.available &&
          root.indexedGeneration === 0 &&
          root.confirmedCompletedSources === 0 &&
          root.unverifiedLegacySources === 0,
      );
      if (
        capabilities !== undefined &&
        attrs.semanticSourceIds.length === 0 &&
        pendingRoots.length > 0
      ) {
        const folders = pendingRoots
          .map((root) => `“${lastPathSegment(root.location) ?? root.label}”`)
          .join(', ');
        return m(
          'p.fm-knowledge-warning',
          { role: 'status' },
          pendingRoots.length === 1
            ? t(
                'knowledgeSearch',
                hasSearchableSources(attrs) ? 'searchIndexPending' : 'searchIndexStarting',
                { folder: folders },
              )
            : t(
                'knowledgeSearch',
                hasSearchableSources(attrs) ? 'searchIndexPendingMany' : 'searchIndexStartingMany',
                { folders },
              ),
        );
      }
      if (capabilities !== undefined && !hasSearchableSources(attrs)) {
        return m(
          'p.fm-knowledge-warning',
          { role: 'status' },
          t('knowledgeSearch', 'searchNoSources'),
        );
      }
      return m('p.fm-knowledge-status', { role: 'status' }, notice ?? t('knowledgeSearch', 'idle'));
    }
    if (result.evidence.length === 0) {
      return m('.fm-knowledge-empty', [
        m('p', { role: 'status' }, t('knowledgeSearch', 'empty')),
        m('p.fm-knowledge-hint', t('knowledgeSearch', 'emptyHint')),
      ]);
    }
    const context = evidenceContext(attrs);
    return m(
      'ol.fm-knowledge-document-list',
      groupEvidenceByDocument(result.evidence).map((group, index) =>
        evidenceDocument(context, group, index),
      ),
    );
  }

  /**
   * The rows the search displayed, keyed by the identity a citation must
   * match. A citation is only ever opened through an identity that is in here,
   * so an answer can never navigate to something the user did not inspect.
   */
  function displayedEvidence(): ReadonlyMap<string, KnowledgeEvidence> {
    return new Map((result?.evidence ?? []).map((row) => [row.recordId, row]));
  }

  /** Copies the answer as plain text, with its numbered source list appended. */
  async function copyAnswer(): Promise<void> {
    const current = answer;
    if (current === undefined) return;
    const numbered = numberKnowledgeAnswer(current.text, current.citations, displayedEvidence());
    const sources = numbered.references.map(
      (reference) => `[${reference.number}] ${reference.title}`,
    );
    const heading = t('knowledgeSearch', 'answerCitations', { count: sources.length });
    try {
      await copyText(
        sources.length === 0
          ? numbered.text
          : `${numbered.text}\n\n${heading}\n${sources.join('\n')}`,
      );
    } catch {
      answerCopied = false;
      answerCopyError = t('knowledgeSearch', 'copyFailed');
      m.redraw();
      return;
    }
    if (answerCopyResetTimer !== undefined) clearTimeout(answerCopyResetTimer);
    answerCopyError = undefined;
    answerCopied = true;
    m.redraw();
    answerCopyResetTimer = window.setTimeout(() => {
      answerCopied = false;
      answerCopyResetTimer = undefined;
      m.redraw();
    }, 1500);
  }

  /**
   * The optional ask bar, docked below the results it is about.
   *
   * Search remains available independently of generation; a host with Settings
   * exposes the reason Ask cannot run rather than silently removing its bar.
   */
  function answerView(attrs: KnowledgeSearchDialogAttrs): m.Children {
    if (!answerAvailable()) {
      if (
        attrs.onConfigureAnswers === undefined ||
        capabilities === undefined ||
        (!capabilities.fullText && !capabilities.semantic)
      )
        return undefined;
      return m(
        'section.fm-knowledge-answer',
        { 'aria-label': t('knowledgeSearch', 'answerRegion') },
        m('.fm-knowledge-ask-bar.fm-knowledge-ask-unavailable', [
          sparklesIcon({ className: 'fm-knowledge-ask-icon', size: 15 }),
          m(
            'span',
            { role: 'status' },
            t(
              'knowledgeSearch',
              answerProfilesFailed ? 'answerProfilesFailed' : 'answerProfileNeeded',
            ),
          ),
          m(
            'button.fm-knowledge-configure-answers',
            {
              type: 'button',
              onclick: attrs.onConfigureAnswers,
            },
            t('knowledgeSearch', 'configureAnswers'),
          ),
        ]),
      );
    }
    const displayed = displayedEvidence();
    const profile = answerProfile();
    const hasOutput =
      generatingAnswer ||
      answer !== undefined ||
      answerError !== undefined ||
      answerNotice !== undefined;
    return m(
      'section.fm-knowledge-answer',
      {
        'aria-label': t('knowledgeSearch', 'answerRegion'),
        'aria-busy': generatingAnswer ? 'true' : 'false',
      },
      [
        profile === undefined ||
        profileModelNotice(profile, modelStatuses.get(profile.id)) === undefined
          ? undefined
          : m(
              '.fm-field-error.fm-knowledge-answer-profile-notice',
              { role: 'alert' },
              profileModelNotice(profile, modelStatuses.get(profile.id)),
            ),
        m('.fm-knowledge-answer-output', { class: hasOutput ? undefined : 'is-empty' }, [
          m('.fm-knowledge-answer-output-heading', [
            m('h3', t('knowledgeSearch', 'answerHeading')),
            answer === undefined || generatingAnswer
              ? undefined
              : tooltip(
                  answerCopied
                    ? t('knowledgeSearch', 'answerCopied')
                    : t('knowledgeSearch', 'copyAnswer'),
                  m(
                    IconButton,
                    {
                      type: 'button',
                      className: 'fm-knowledge-answer-copy',
                      'aria-label': answerCopied
                        ? t('knowledgeSearch', 'answerCopied')
                        : t('knowledgeSearch', 'copyAnswer'),
                      onclick: () => void copyAnswer(),
                    },
                    answerCopied ? checkIcon({ size: 14 }) : copyIcon({ size: 14 }),
                  ),
                ),
            m(
              'button.fm-knowledge-answer-collapse',
              {
                type: 'button',
                'aria-label': answerCollapsed
                  ? t('knowledgeSearch', 'showAnswer')
                  : t('knowledgeSearch', 'hideAnswer'),
                'aria-expanded': answerCollapsed ? 'false' : 'true',
                'aria-controls': 'fm-knowledge-answer-body',
                onclick: () => {
                  answerCollapsed = !answerCollapsed;
                },
              },
              answerCollapsed ? chevronDownIcon({ size: 16 }) : chevronUpIcon({ size: 16 }),
            ),
          ]),
          answerCopied
            ? m('span.fm-visually-hidden', { role: 'status' }, t('knowledgeSearch', 'answerCopied'))
            : undefined,
          answerCopyError === undefined
            ? undefined
            : m('p.fm-knowledge-error', { role: 'alert' }, answerCopyError),
          answerError === undefined
            ? undefined
            : m('p.fm-knowledge-error', { role: 'alert' }, answerError),
          m(
            '.fm-knowledge-answer-body#fm-knowledge-answer-body',
            {
              'aria-live': 'polite',
              hidden: answerCollapsed,
            },
            hasOutput
              ? knowledgeAnswerBody({
                  answer,
                  generating: generatingAnswer,
                  notice: answerNotice,
                  displayed,
                  canOpen: attrs.onOpenSource !== undefined,
                  openRow: (row) => void openSource(attrs, row),
                })
              : undefined,
          ),
        ]),
        knowledgeAskBar({
          question: questionText,
          allowModelKnowledge,
          generating: generatingAnswer,
          profile,
          canAsk: canGenerateAnswer(attrs),
          onQuestion: (value) => {
            // The previous answer stays until the new question is actually asked.
            questionText = value;
          },
          onAllowModelKnowledge: (value) => {
            allowModelKnowledge = value;
            resetAnswer();
          },
          onAsk: () => void ask(attrs),
          onCancel: cancelAnswer,
        }),
      ],
    );
  }

  return {
    oninit: ({ attrs }) => {
      currentAttrs = attrs;
      wasOpen = attrs.open;
      if (attrs.open) void load(attrs);
      rootRefreshTimer = setInterval(() => void refreshRoots(currentAttrs), 3_000);
      if (attrs.client.connection.get() !== 'closed') {
        void attrs.client
          .subscribe((event) => {
            if (event.payload.type === 'semantic.ingestionProgress' && !removed) {
              void refreshRoots(currentAttrs);
            }
          })
          .then((unsubscribe) => {
            if (removed) unsubscribe();
            else rootRefreshUnsubscribe = unsubscribe;
          })
          .catch(() => {
            if (!removed) {
              rootRefreshError = t('knowledgeSearch', 'loadFailed');
              m.redraw();
            }
          });
      }
    },
    onupdate: ({ attrs }) => {
      currentAttrs = attrs;
      if (attrs.open && (!wasOpen || openedWorkspaceId !== attrs.workspaceId)) void load(attrs);
      wasOpen = attrs.open;
      if (attrs.open && loadedProfileRevision !== (attrs.profileRevision ?? 0)) {
        loadedProfileRevision = attrs.profileRevision ?? 0;
        void refreshAnswerProfiles(attrs);
      }
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
    onremove: () => {
      removed = true;
      if (rootRefreshTimer !== undefined) clearInterval(rootRefreshTimer);
      if (answerCopyResetTimer !== undefined) clearTimeout(answerCopyResetTimer);
      rootRefreshUnsubscribe?.();
      abortController?.abort();
    },
    view: ({ attrs }) => {
      const offline = attrs.client.connection.get() === 'closed';
      const searchable = canSearch(attrs);
      if (!attrs.open) return undefined;
      return m(
        'section#fm-knowledge-search-pane.fm-knowledge-search',
        {
          'aria-label': t('knowledgeSearch', 'title'),
        },
        [
          m('.fm-knowledge-composer', [
            m('.fm-knowledge-search-toolbar', [
              filterIcon({ className: 'fm-knowledge-search-icon', size: 14 }),
              m(
                'select.fm-knowledge-intent.browser-default',
                {
                  name: 'knowledge-intent',
                  value: intent,
                  disabled: busy === 'searching',
                  'aria-label': t('knowledgeSearch', 'intent'),
                  onchange: (event: Event) => {
                    intent = (event.currentTarget as HTMLSelectElement)
                      .value as KnowledgeQueryIntent;
                    edited();
                  },
                },
                INTENTS.map((candidate) =>
                  m('option', { key: candidate, value: candidate }, intentLabel(candidate)),
                ),
              ),
              m('textarea#fm-knowledge-subjects', {
                ...technicalTextInputAttrs,
                name: 'knowledge-subjects',
                rows: 1,
                value: subjectsText,
                disabled: busy === 'searching',
                'aria-label': t('knowledgeSearch', 'subjects'),
                placeholder: t('knowledgeSearch', 'subjectsPlaceholder'),
                onupdate: ({ dom }: m.VnodeDOM) => {
                  if (!focusSubjectOnOpen) return;
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
                    if (busy === 'loading') {
                      searchAfterLoad = true;
                      return;
                    }
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
                    disabled: busy !== undefined || !canSearch(attrs),
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
                  dotsIcon({ size: 16 }),
                ),
              ),
              tooltip(
                t('knowledgeSearch', 'close'),
                m(
                  IconButton,
                  {
                    type: 'button',
                    className: 'fm-knowledge-search-close',
                    'aria-label': t('knowledgeSearch', 'close'),
                    onclick: () => close(attrs),
                  },
                  closeIcon({ size: 13 }),
                ),
              ),
            ]),
            openingFolder === undefined
              ? undefined
              : m('label.fm-knowledge-folder-filter', [
                  m('input', {
                    type: 'checkbox',
                    checked: scopeKind === 'currentFolder',
                    disabled: !currentFolderIndexed || busy === 'searching',
                    onchange: (event: Event) => {
                      const checked = (event.currentTarget as HTMLInputElement).checked;
                      pinnedFolder = checked ? openingFolder : undefined;
                      scopeKind = checked ? 'currentFolder' : 'enrolledRoots';
                      edited();
                      void reinterpret(attrs);
                    },
                  }),
                  m(
                    'span',
                    t('knowledgeSearch', 'includeOnlyCurrentFolder', {
                      folder: lastPathSegment(openingFolder) ?? pathFromUri(openingFolder.uri),
                    }),
                  ),
                ]),
            !currentFolderIndexed && openingFolder !== undefined && busy !== 'loading'
              ? m(
                  'button.fm-knowledge-include-folder-action',
                  {
                    type: 'button',
                    onclick: () => {
                      enrolmentOpen = true;
                    },
                  },
                  t('knowledgeSearch', 'includeFolder'),
                )
              : undefined,
            openingFolder === undefined
              ? undefined
              : m(ModalPanel, {
                  className: 'fm-dense-modal fm-semantic-enrolment-modal',
                  title: t('ragAsk', 'includeFolderTitle'),
                  isOpen: enrolmentOpen,
                  closeOnEsc: true,
                  onToggle: (open: boolean) => {
                    enrolmentOpen = open;
                  },
                  description: enrolmentOpen
                    ? m(SemanticFolderEnrolmentPrompt, {
                        client: attrs.client,
                        workspaceId: attrs.workspaceId,
                        location: openingFolder,
                        onEnrolled: () => {
                          currentFolderIndexed = true;
                          scopeKind = 'enrolledRoots';
                          enrolmentOpen = false;
                          void refreshRoots(attrs).then(() => {
                            const included = roots.find(
                              (root) =>
                                root.location.providerId === openingFolder?.providerId &&
                                root.location.uri === openingFolder.uri,
                            );
                            if (included !== undefined) selectedRootIds.add(included.rootId);
                          });
                          m.redraw();
                        },
                        onCancel: () => {
                          enrolmentOpen = false;
                        },
                      })
                    : undefined,
                }),
            m(ModalPanel, {
              className: 'fm-dense-modal fm-knowledge-settings-modal',
              title: t('knowledgeSearch', 'settings'),
              isOpen: settingsOpen,
              closeOnEsc: true,
              onToggle: (open: boolean) => {
                settingsOpen = open;
              },
              description: m('.fm-knowledge-settings', [
                m('details.fm-knowledge-advanced', [
                  m('summary', t('knowledgeSearch', 'showAdvanced')),
                  m('.fm-knowledge-advanced-body', [
                    m('fieldset.fm-knowledge-inline-needs.fm-knowledge-needs', [
                      m('legend', t('knowledgeSearch', 'searchFor')),
                      NEEDS.map((need) =>
                        m('label', { key: need }, [
                          m('input', {
                            type: 'checkbox',
                            checked: (draft.needs ?? []).includes(need),
                            disabled: busy === 'searching',
                            onchange: (event: Event) =>
                              updateNeed(
                                attrs,
                                need,
                                (event.currentTarget as HTMLInputElement).checked,
                              ),
                          }),
                          m('span', needLabel(need)),
                        ]),
                      ),
                    ]),
                    m('.fm-knowledge-row', [
                      scopeKind === 'currentFolder'
                        ? undefined
                        : m('label.fm-knowledge-field', [
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
                                  {
                                    key: kind,
                                    value: kind,
                                    disabled: !scopeAvailable(attrs, kind),
                                  },
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
                          roots.some(
                            (root) => root.pdfImagesEnabled && root.pdfImagePageBudget !== null,
                          )
                            ? m(
                                'p.fm-knowledge-hint',
                                { role: 'status' },
                                t('knowledgeSearch', 'pdfImagesLimitedNotice', {
                                  pages: Math.min(
                                    ...roots
                                      .filter(
                                        (root) =>
                                          root.pdfImagesEnabled && root.pdfImagePageBudget !== null,
                                      )
                                      .map((root) => root.pdfImagePageBudget)
                                      .filter((budget): budget is number => budget !== null),
                                  ),
                                }),
                              )
                            : undefined,
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
                        ...technicalTextInputAttrs,
                        name: 'knowledge-dsl',
                        rows: 4,
                        value: dslText,
                        disabled: busy === 'searching',
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
                        knowledgeTraceView(result, traceRequested),
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
                          : knowledgePlanBody(plan),
                      ],
                    ),
                    result === undefined ? undefined : knowledgeResultDetails(result),
                  ]),
                ]),
              ]),
            }),
          ]),
          error === undefined ? undefined : m('p.fm-knowledge-error', { role: 'alert' }, error),
          rootRefreshError === undefined
            ? undefined
            : m('p.fm-knowledge-error', { role: 'alert' }, rootRefreshError),
          offline
            ? m('p.fm-knowledge-warning', { role: 'status' }, t('knowledgeSearch', 'offline'))
            : undefined,
          busy === 'loading'
            ? m('p.fm-knowledge-status', { role: 'status' }, t('knowledgeSearch', 'loading'))
            : undefined,
          m('.fm-knowledge-workspace', [
            m(
              'section.fm-knowledge-results-section.fm-knowledge-evidence-panel',
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
                m('.fm-knowledge-results-body', { 'aria-live': 'polite' }, [resultsView(attrs)]),
              ],
            ),
            answerView(attrs),
          ]),
        ],
      );
    },
  };
};

/** Compatibility export for task 0206 callers while the pane replaces the former modal. */
export const KnowledgeSearchDialog = KnowledgeSearchPane;
