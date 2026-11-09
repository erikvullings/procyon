import { DEFAULT_MAP_SETTINGS } from '../../features/map/map-settings';
import { DEFAULT_FILE_TEMPLATES } from '../../features/templates/file-templates';
import { apiErrorMessage } from '../../i18n';
import type {
  BackendEvent,
  ChecksumAlgorithm,
  ChecksumEntry,
  ComparisonCriteria,
  ComparisonEntry,
  Connection,
  ConnectionId,
  DirectorySnapshot,
  DocumentSummary,
  DocxPreviewResource,
  DuplicateGroup,
  EntrySummary,
  ExecuteKnowledgeSearchRequest,
  FinderTags,
  FrontendDiagnostic,
  InterruptedTransfer,
  KnowledgeScope,
  KnowledgeSearchPlan,
  ListDirectoryRequest,
  LlmProfile,
  Location,
  OneDriveAuthorizationAttempt,
  OpenStructuredViewRequest,
  Operation,
  OperationId,
  PlanKnowledgeSearchRequest,
  PlanSemanticExclusionRequest,
  PluginDescriptor,
  PreviewSemanticEnrolmentRequest,
  SavedRagConversation,
  SearchInFileRequest,
  SemanticComponentLifecycle,
  SemanticComponentStatus,
  SemanticIndexRemovalPlan,
  SemanticInstallationOffer,
  SemanticModelIdentity,
  SemanticModelMigrationPlan,
  SemanticOcrJob,
  SemanticOcrStatus,
  SemanticOcrTarget,
  SemanticProfile,
  SemanticRootStatus,
  SemanticVocabulary,
  Settings,
  SpotlightComment,
  StartSnapshotJobRequest,
  StructuredView,
  WorkspaceId,
  WorkspaceProjection,
} from '../../models';
import { defaultKnowledgeSearchOptions } from '../../models';
import { EventStreamSignalRegistry, MutableEventStreamStatus } from '../events/event-stream';
import type { MockConnectionClient } from './mock-connection-client';
import type { GeneratedDirectorySize } from './mock-directory-generator';
import { createGeneratedDirectory, GENERATED_DIRECTORY_SIZES } from './mock-directory-generator';
import {
  MockKnowledgeEvidenceCache,
  MockKnowledgeScopeError,
  planMockKnowledgeSearch,
  resolveMockKnowledgeScope,
} from './mock-knowledge-search';
import type { MockOperationClient } from './mock-operation-client';
import type { MockPluginClient } from './mock-plugin-client';
import type { MockSemanticClient } from './mock-semantic-client';
import type { MockClientMethod, MockFileManagerClientOptions } from './mock-shared';
import {
  aggregateTotals,
  directories,
  fixtureEntry,
  MOCK_SEMANTIC_ENROLMENT_INVENTORY,
  MockClientError,
  mockSemanticLibraryStatus,
  mockSemanticOcrStatus,
  mockSemanticOffer,
  mockSemanticSelection,
  mockSemanticStatus,
  parseMockDelimited,
  plugins,
  sameSemanticOcrTarget,
  semanticLocationContains,
  syntheticFileContent,
} from './mock-shared';
import type { MockSqliteDatabase } from './mock-sqlite-database';
import type { MockWorkspaceClient } from './mock-workspace-client';

export class MockContext {
  readonly connection = new MutableEventStreamStatus();

  readonly supportsAppUpdates = false;

  readonly supportsAppDataRemoval = false;

  readonly frontendDiagnostics: FrontendDiagnostic[] = [];

  readonly resynchronise = new EventStreamSignalRegistry();

  readonly pageSize: number;

  readonly seed: number;

  readonly loadingLocations: ReadonlySet<string>;

  readonly latencyMs: number;

  readonly failures: Partial<Record<MockClientMethod, Error>>;

  readonly nativeIconExtensions: ReadonlySet<string>;

  semanticStatus: SemanticComponentStatus;

  semanticOcrStatus: SemanticOcrStatus;

  readonly semanticOcrAutoAdvance: boolean;

  semanticOcrJobSequence = 0;

  readonly semanticOcrJobTargets = new Map<string, readonly SemanticOcrTarget[]>();

  semanticOfferSequence = 0;

  semanticIndexRemovalSequence = 0;

  semanticMigrationSequence = 0;

  readonly semanticOffers = new Map<string, SemanticInstallationOffer>();

  readonly semanticEnrolmentInventory = new Map(
    Object.entries(MOCK_SEMANTIC_ENROLMENT_INVENTORY).map(([enrolmentId, counts]) => [
      enrolmentId,
      structuredClone(counts),
    ]),
  );

  readonly semanticIndexRemovalPlans = new Map<string, SemanticIndexRemovalPlan>();

  readonly semanticMigrationPlans = new Map<string, SemanticModelMigrationPlan>();

  semanticLibrary = mockSemanticLibraryStatus();

  semanticVocabularies: SemanticVocabulary[] = [];

  semanticLibrarySequence = 0;

  readonly semanticEnrolmentPreviews = new Map<
    string,
    PreviewSemanticEnrolmentRequest & { readonly policyRevision: number }
  >();

  readonly semanticExclusionPlans = new Map<
    string,
    PlanSemanticExclusionRequest & { readonly rootId: string }
  >();

  readonly finderTagsByUri = new Map<string, FinderTags>();

  readonly spotlightCommentsByUri = new Map<string, SpotlightComment>();

  readonly listeners = new Set<(event: BackendEvent) => void>();

  readonly scriptedEvents: BackendEvent[] = [];

  readonly operations = new Map<OperationId, Operation>();
  /** Transfers a pretend crash left behind; empty unless a test or demo seeds it. */
  readonly interruptedTransfers = new Map<OperationId, InterruptedTransfer>();

  readonly navigationHistory = new Map<string, { back: Location[]; forward: Location[] }>();

  readonly workspaces = new Map<WorkspaceId, WorkspaceProjection>();

  readonly connections = new Map<ConnectionId, Connection>();

  readonly llmProfiles = new Map<string, LlmProfile>();

  readonly documentSummaries = new Map<string, DocumentSummary>();

  readonly ragConversations = new Map<string, SavedRagConversation>();

  readonly knowledgeSearches = new Map<string, AbortController>();

  readonly knowledgeAnswers = new Map<string, AbortController>();

  /** Inspected evidence sets an optional answer may be generated from. */
  readonly knowledgeEvidence = new MockKnowledgeEvidenceCache();

  readonly ephemeralRagConversations = new Map<string, SavedRagConversation>();

  readonly oneDriveAuthorizations = new Map<
    string,
    { readonly connectionId: ConnectionId; attempt: OneDriveAuthorizationAttempt }
  >();

  pluginState: PluginDescriptor[] = structuredClone(plugins);

  settings: Settings = {
    schemaVersion: 8,
    theme: 'auto',
    language: 'en',
    fontSize: 13,
    rowHeight: 20,
    dateFormat: 'iso',
    sizeFormat: 'binary',
    showHiddenFiles: false,
    compactToolbar: true,
    confirmPermanentDelete: true,
    confirmFileOperations: true,
    defaultConflictPolicy: 'ask',
    operationConcurrency: 2,
    defaultPaneLayout: 'dual',
    defaultColumns: ['core.name', 'core.extension', 'core.size', 'core.modified'],
    columnWidths: {},
    keybindings: {},
    enabledPlugins: [],
    pluginSettings: {},
    terminalCommand: null,
    terminalApps: [{ name: 'Terminal', command: 'Terminal' }],
    fileTemplates: DEFAULT_FILE_TEMPLATES.map((template) => ({ ...template })),
    editorCommand: null,
    defaultStartLocations: [],
    favouriteLocations: [],
    recentLocationsByWorkspace: {},
    multiRenamePresets: [],
    filterPresets: [],
    syncProfiles: [],
    savedSearches: [],
    iconTheme: 'generic',
    checkForUpdatesAutomatically: true,
    checkForBetaUpdates: false,
    viewerInSeparateWindow: false,
    semanticMaxSourceMib: 64,
    semanticThemeRootIds: [],
    quickFilterMatchMode: 'fuzzy',
    rememberFolderViews: true,
    map: DEFAULT_MAP_SETTINGS,
  };

  operationSequence = 0;

  tabSequence = 0;

  workspaceSequence = 0;

  connectionSequence = 0;

  llmProfileSequence = 0;

  /** Mirrors the backend: the first profile saved becomes active until another is activated. */
  activeLlmProfileId: string | undefined;

  oneDriveAuthorizationSequence = 0;

  searchSequence = 0;

  eventSequence = 0;

  readonly searches = new Map<string, { cancelled: boolean; entries: readonly EntrySummary[] }>();

  comparisonSequence = 0;

  readonly comparisons = new Map<
    string,
    {
      cancelled: boolean;
      entries: readonly ComparisonEntry[];
      left: Location;
      right: Location;
      criteria: ComparisonCriteria;
    }
  >();

  checksumSequence = 0;

  readonly checksumJobs = new Map<
    string,
    {
      cancelled: boolean;
      entries: readonly ChecksumEntry[];
      algorithms: readonly ChecksumAlgorithm[];
    }
  >();

  readonly snapshotJobs = new Map<string, StartSnapshotJobRequest>();

  readonly linkScans = new Map<
    string,
    { readonly roots: readonly Location[]; cancelled: boolean }
  >();

  readonly musicScans = new Map<
    string,
    { readonly roots: readonly Location[]; cancelled: boolean }
  >();

  duplicateScanSequence = 0;

  readonly duplicateScans = new Map<
    string,
    { cancelled: boolean; groups: readonly DuplicateGroup[]; roots: readonly Location[] }
  >();

  readonly fileContents = new Map<string, Uint8Array>();

  readonly docxSessions = new Map<string, Map<string, DocxPreviewResource>>();

  readonly pptxSessions = new Map<string, Uint8Array>();

  readonly sqliteSessions = new Map<string, MockSqliteDatabase>();

  readonly structuredSessions = new Map<
    string,
    {
      uri: string;
      format: OpenStructuredViewRequest['format'];
      delimiter: string;
      headerMode: NonNullable<OpenStructuredViewRequest['headerMode']>;
      selectedSheet: string;
    }
  >();

  // Generated directories are recreated per request, but their aggregate totals are a pure
  // function of (size, seed) — cache them instead of resumming up to 1,000,000 entries on every
  // paginated fetch.
  readonly generatedTotalsCache = new Map<
    GeneratedDirectorySize,
    { readonly size: number; readonly fileCount: number }
  >();

  connectionClient!: MockConnectionClient;
  workspaceClient!: MockWorkspaceClient;
  operationClient!: MockOperationClient;
  semanticClient!: MockSemanticClient;
  pluginClient!: MockPluginClient;

  constructor(options: MockFileManagerClientOptions = {}) {
    this.pageSize = options.pageSize ?? 100;
    this.seed = options.seed ?? 13;
    this.loadingLocations = new Set(options.loadingLocations);
    this.latencyMs = options.latencyMs ?? 0;
    this.failures = options.failures ?? {};
    this.nativeIconExtensions = new Set(
      options.nativeIconExtensions?.map((extension) => extension.toLowerCase()),
    );
    const semanticLifecycle = options.semanticLifecycle ?? 'absent';
    this.semanticStatus = mockSemanticStatus(semanticLifecycle);
    this.semanticOcrStatus = structuredClone(options.semanticOcrStatus ?? mockSemanticOcrStatus());
    this.semanticOcrAutoAdvance = options.semanticOcrStatus === undefined;
    if (semanticLifecycle === 'offered') {
      this.semanticOffers.set(
        'mock-scenario-offer',
        mockSemanticOffer('mock-scenario-offer', 'compactMultilingual'),
      );
    }
    if (semanticLifecycle === 'migrating') {
      const progress = this.semanticStatus.migration;
      if (progress !== null && progress !== undefined) {
        this.semanticMigrationPlans.set(progress.migrationId, {
          migrationId: progress.migrationId,
          from: mockSemanticSelection('compactMultilingual'),
          target: progress.target,
          estimate: progress.estimate,
          reason: progress.reason,
          fullReindex: true,
          requiresConfirmation: true,
          resumable: true,
        });
      }
    }
  }

  requireWorkspaceRevision(workspace: WorkspaceProjection, expectedRevision: number): void {
    if (workspace.revision !== expectedRevision) {
      throw new MockClientError(
        'workspaceRevisionConflict',
        'The workspace changed after this view was loaded.',
      );
    }
  }

  /** Simulates a replay gap requiring affected panes to refetch. */
  emitResynchronise(): void {
    this.resynchronise.dispatch();
  }

  /** Replaces the pending event script; call {@link emitNextEvent} to advance it. */
  scriptEvents(events: readonly BackendEvent[]): void {
    this.scriptedEvents.splice(0, this.scriptedEvents.length, ...structuredClone(events));
  }

  /** Delivers one pending scripted event to every active subscriber. */
  emitNextEvent(): boolean {
    const event = this.scriptedEvents.shift();
    if (event === undefined) {
      return false;
    }
    this.emit(event);
    return true;
  }

  /** Delivers an event immediately to every active subscriber. */
  emit(event: BackendEvent): void {
    for (const listener of this.listeners) {
      listener(structuredClone(event));
    }
  }

  knowledgePlan(
    request: PlanKnowledgeSearchRequest | ExecuteKnowledgeSearchRequest,
  ): KnowledgeSearchPlan {
    if ((request.draft.about ?? []).filter((subject) => subject.trim().length > 0).length === 0) {
      throw new MockClientError('invalidRequest', 'A knowledge search needs at least one subject', {
        reason: 'semanticQueryEmpty',
      });
    }
    return planMockKnowledgeSearch(
      request.draft,
      this.knowledgeScope(request),
      request.mode ?? 'hybrid',
      request.options ?? defaultKnowledgeSearchOptions(),
    );
  }

  /** Resolves the visible scope, refusing exactly what the backend refuses. */
  knowledgeScope(
    request: PlanKnowledgeSearchRequest | ExecuteKnowledgeSearchRequest,
  ): KnowledgeScope {
    try {
      return resolveMockKnowledgeScope(request.scope, request.draft);
    } catch (error) {
      if (error instanceof MockKnowledgeScopeError) {
        throw new MockClientError(error.code, error.message);
      }
      throw error;
    }
  }

  requireConnection(connectionId: ConnectionId): Connection {
    const connection = this.connections.get(connectionId);
    if (connection === undefined) {
      throw new MockClientError('notFound', `No mock connection with id ${connectionId}`);
    }
    return connection;
  }

  requireLlmProfile(profileId: string): LlmProfile {
    const profile = this.llmProfiles.get(profileId);
    if (profile === undefined) {
      throw new MockClientError('notFound', `No mock LLM profile with id ${profileId}`);
    }
    return profile;
  }

  /** Returns the current in-memory state for a mock operation. */
  getOperation(operationId: OperationId): Operation | undefined {
    const operation = this.operations.get(operationId);
    return operation === undefined ? undefined : structuredClone(operation);
  }

  structuredSession(sessionId: string) {
    const session = this.structuredSessions.get(sessionId);
    if (session === undefined) {
      throw new MockClientError('notFound', 'structured viewer session not found');
    }
    return session;
  }

  mockStructuredView(sessionId: string): StructuredView {
    const session = this.structuredSession(sessionId);
    const bytes = this.fileContentFor(session.uri);
    const externalFallback = session.format === 'excel' && bytes.length > 16 * 1024 * 1024;
    const jsonText = session.format === 'json';
    const workbookRecords =
      session.selectedSheet === 'Details'
        ? [['Details'], [], ['Sparse row']]
        : [
            ['Label', 'Cached formula'],
            ['Summary', '84'],
          ];
    const records =
      session.format === 'excel' && !externalFallback
        ? workbookRecords
        : externalFallback || jsonText
          ? []
          : session.format === 'ndjson'
            ? new TextDecoder()
                .decode(bytes)
                .split(/\r?\n/)
                .filter(Boolean)
                .map((line) => [line])
            : parseMockDelimited(
                new TextDecoder().decode(bytes).replace(/^\uFEFF/, ''),
                session.delimiter,
              );
    const useHeader =
      session.format !== 'excel' &&
      (session.headerMode === 'firstRow' || session.headerMode === 'auto');
    const headers = useHeader
      ? (records[0] ?? [])
      : (records[0]?.map((_, index) => `Column ${index + 1}`) ?? []);
    const dataRecords = useHeader ? records.slice(1) : records;
    const rows = dataRecords.slice(0, 500).map((cells, index) => ({
      index,
      cells,
      ...(session.format === 'excel' && session.selectedSheet === 'Summary' && index === 1
        ? {
            cellDetails: [
              {
                column: 1,
                display: '84',
                valueType: 'number' as const,
                formula: 'B1*2',
              },
            ],
          }
        : {}),
    }));
    return {
      sessionId,
      kind: externalFallback ? 'externalFallback' : jsonText ? 'jsonText' : 'table',
      sourceRevision: String(bytes.length),
      sourceBytes: bytes.length,
      randomAccess: true,
      delimiter:
        externalFallback || jsonText || session.format === 'ndjson' ? null : session.delimiter,
      headerMode: session.headerMode,
      headers,
      rows,
      ...(session.format === 'excel' && !externalFallback
        ? {
            sheets: [
              { name: 'Summary', rowCount: 2, columnCount: 2 },
              { name: 'Details', rowCount: 3, columnCount: 1 },
            ],
            selectedSheet: session.selectedSheet,
          }
        : {}),
      indexedBytes: bytes.length,
      indexedRows: dataRecords.length,
      totalRows: dataRecords.length,
      indexingComplete: true,
      warning: externalFallback
        ? "This workbook cannot be opened within the viewer's bounded-memory budget. Open it in an external spreadsheet application."
        : null,
    };
  }

  directorySnapshot(
    request: ListDirectoryRequest,
    signal: AbortSignal | undefined,
    method: 'navigatePane' | 'listDirectory',
  ): Promise<DirectorySnapshot> {
    const fixtures = directories[request.location.uri];
    const generatedSize = this.generatedSize(request.location.uri);
    const searchId = request.location.uri.startsWith('search://local/')
      ? request.location.uri.slice('search://local/'.length)
      : undefined;
    const searchEntries = searchId === undefined ? undefined : this.searches.get(searchId)?.entries;
    if (fixtures === undefined && generatedSize === undefined && searchEntries === undefined) {
      return Promise.reject(
        new MockClientError('notFound', `No mock directory at ${request.location.uri}`),
      );
    }

    const offset = this.parseContinuationToken(request.continuationToken);
    const entries =
      searchEntries !== undefined
        ? searchEntries.slice(offset, offset + this.pageSize)
        : generatedSize === undefined
          ? (fixtures ?? []).map((fixture) => fixtureEntry(request.location.uri, fixture))
          : createGeneratedDirectory(generatedSize, this.seed).page(offset, this.pageSize);
    const totalEntries = searchEntries?.length ?? generatedSize ?? fixtures?.length ?? 0;
    const { size: totalKnownSize, fileCount: totalKnownFileCount } =
      generatedSize === undefined
        ? aggregateTotals(entries)
        : this.generatedDirectoryTotals(generatedSize);
    const nextOffset = offset + entries.length;
    const isUnreadable = request.location.uri === 'mock:///Unreadable';
    const loadingState = isUnreadable
      ? ({
          type: 'error',
          message: apiErrorMessage('permissionDenied') ?? 'Permission denied.',
        } as const)
      : this.loadingLocations.has(request.location.uri)
        ? ({ type: 'loading' } as const)
        : ({ type: 'loaded' } as const);
    // A plausible synthetic capacity so the status bar's "available" segment is
    // exercisable in mock mode; omitted for search results, which mirror the real
    // backend's non-local-provider gap (no backing volume to report).
    const volumeCapacity =
      searchId === undefined
        ? { totalBytes: 2_000_000_000_000, availableBytes: 616_040_000_000 }
        : undefined;

    return this.perform(method, signal, () => ({
      paneId: request.paneId,
      requestId: request.requestId,
      revision: 1,
      location: request.location,
      writable: request.location.uri !== 'mock:///Read-only',
      entries: isUnreadable ? [] : entries,
      totalKnownEntries: totalEntries,
      totalKnownSize,
      totalKnownFileCount,
      hasMore: nextOffset < totalEntries,
      ...(nextOffset < totalEntries ? { continuationToken: String(nextOffset) } : {}),
      loadingState,
      ...(volumeCapacity === undefined ? {} : { volumeCapacity }),
    }));
  }

  generatedDirectoryTotals(size: GeneratedDirectorySize): {
    size: number;
    fileCount: number;
  } {
    const cached = this.generatedTotalsCache.get(size);
    if (cached !== undefined) {
      return cached;
    }
    const totals = aggregateTotals(createGeneratedDirectory(size, this.seed).entries());
    this.generatedTotalsCache.set(size, totals);
    return totals;
  }

  generatedSize(uri: string): GeneratedDirectorySize | undefined {
    const match = /^mock:\/\/\/large\/(\d+)$/.exec(uri);
    if (match?.[1] === undefined) {
      return undefined;
    }
    const size = Number(match[1]);
    return GENERATED_DIRECTORY_SIZES.find((candidate) => candidate === size);
  }

  parseContinuationToken(token: string | undefined): number {
    if (token === undefined) {
      return 0;
    }
    const offset = Number(token);
    if (!Number.isSafeInteger(offset) || offset < 0) {
      throw new MockClientError('invalidContinuationToken', `Invalid continuation token: ${token}`);
    }
    return offset;
  }

  requireOperation(operationId: OperationId): Operation {
    const operation = this.operations.get(operationId);
    if (operation === undefined) {
      throw new MockClientError('notFound', `No mock operation with id ${operationId}`);
    }
    return operation;
  }

  requireChecksumJob(jobId: string): {
    cancelled: boolean;
    entries: readonly ChecksumEntry[];
    algorithms: readonly ChecksumAlgorithm[];
  } {
    const job = this.checksumJobs.get(jobId);
    if (job === undefined) {
      throw new MockClientError('notFound', `No mock checksum job with id ${jobId}`);
    }
    return job;
  }

  requireDuplicateScan(scanId: string): {
    cancelled: boolean;
    groups: readonly DuplicateGroup[];
    roots: readonly Location[];
  } {
    const scan = this.duplicateScans.get(scanId);
    if (scan === undefined) {
      throw new MockClientError('notFound', `No mock duplicate scan with id ${scanId}`);
    }
    return scan;
  }

  requireComparison(comparisonId: string): {
    cancelled: boolean;
    entries: readonly ComparisonEntry[];
    left: Location;
    right: Location;
    criteria: ComparisonCriteria;
  } {
    const comparison = this.comparisons.get(comparisonId);
    if (comparison === undefined) {
      throw new MockClientError('notFound', `No mock comparison with id ${comparisonId}`);
    }
    return comparison;
  }

  fileContentFor(uri: string): Uint8Array {
    let content = this.fileContents.get(uri);
    if (content === undefined) {
      content = syntheticFileContent(uri);
      this.fileContents.set(uri, content);
    }
    return content;
  }

  /** Builds a per-line match finder for a search request, mirroring the backend's
   * substring/regex, case-(in)sensitive `ContentQuery` semantics closely enough for mock/dev use. */
  buildLineMatcher(request: SearchInFileRequest): (line: string) => [number, number][] {
    if (request.regex) {
      const source = request.wholeWord ? `\\b(?:${request.query})\\b` : request.query;
      let pattern: RegExp;
      try {
        pattern = new RegExp(source, request.caseSensitive ? 'gu' : 'giu');
      } catch (error) {
        throw new MockClientError(
          'invalidRequest',
          `invalid regular expression: ${(error as Error).message}`,
          { detail: (error as Error).message },
        );
      }
      return (line) => {
        const found: [number, number][] = [];
        pattern.lastIndex = 0;
        let match = pattern.exec(line);
        while (match !== null) {
          found.push([match.index, match.index + match[0].length]);
          pattern.lastIndex = match[0].length === 0 ? match.index + 1 : pattern.lastIndex;
          match = pattern.exec(line);
        }
        return found;
      };
    }
    const needle = request.caseSensitive ? request.query : request.query.toLowerCase();
    const isWordChar = (char: string | undefined): boolean =>
      char !== undefined && /[A-Za-z0-9_]/u.test(char);
    return (line) => {
      const haystack = request.caseSensitive ? line : line.toLowerCase();
      const found: [number, number][] = [];
      let from = 0;
      let index = haystack.indexOf(needle, from);
      while (index !== -1) {
        const boundaryOk =
          !request.wholeWord ||
          (!isWordChar(line[index - 1]) && !isWordChar(line[index + needle.length]));
        if (boundaryOk) {
          found.push([index, index + needle.length]);
        }
        from = index + needle.length;
        index = haystack.indexOf(needle, from);
      }
      return found;
    };
  }

  requireActiveSemanticFolder(workspaceId: WorkspaceId, location: Location): void {
    const workspace = this.workspaces.get(workspaceId);
    const pane = workspace === undefined ? undefined : workspace.panesById[workspace.activePaneId];
    const tab = pane === undefined ? undefined : pane.tabsById[pane.activeTabId];
    if (
      tab === undefined ||
      tab.location.providerId !== location.providerId ||
      tab.location.uri !== location.uri
    ) {
      throw new MockClientError('workspaceRequired', 'An active workspace/root is required');
    }
  }

  requireActiveOrEnrolledSemanticRoot(workspaceId: WorkspaceId, location: Location): void {
    if (
      this.workspaces.has(workspaceId) &&
      this.semanticLibrary.roots.some(
        (root) =>
          root.location.providerId === location.providerId && root.location.uri === location.uri,
      )
    ) {
      return;
    }
    this.requireActiveSemanticFolder(workspaceId, location);
  }

  requireSemanticLibraryRevision(expected: number): void {
    if (this.semanticLibrary.revision !== expected) {
      throw new MockClientError(
        'staleRevision',
        `Semantic policy changed from revision ${expected} to ${this.semanticLibrary.revision}`,
      );
    }
  }

  semanticRootFor(location: Location): SemanticRootStatus | undefined {
    const matching = this.semanticLibrary.roots
      .filter(
        (root) =>
          root.location.providerId === location.providerId &&
          (root.location.uri === location.uri ||
            (root.recursive && semanticLocationContains(root.location, location))),
      )
      .sort((left, right) => left.location.uri.length - right.location.uri.length);
    const root = matching.at(-1);
    if (
      root?.exclusions.some((exclusion) =>
        semanticLocationContains(exclusion.location, location),
      ) === true
    ) {
      throw new MockClientError('alreadyExcluded', 'The folder is already excluded');
    }
    return root;
  }

  advanceSemanticOcrJobs(): void {
    const active = this.semanticOcrStatus.jobs.find(
      (job) => job.state === 'queued' || job.state === 'running',
    );
    if (active === undefined) return;
    if (active.state === 'queued') {
      this.semanticOcrStatus = {
        ...this.semanticOcrStatus,
        jobs: this.semanticOcrStatus.jobs.map((job) =>
          job.id === active.id ? { ...job, state: 'running', updatedAtMs: Date.now() } : job,
        ),
      };
      return;
    }
    const targets = this.semanticOcrJobTargets.get(active.id) ?? [];
    const completed: SemanticOcrJob = {
      ...active,
      state: 'completed',
      updatedAtMs: Date.now(),
      processedFiles: targets.length,
      files: targets.map((target) => ({
        ...target,
        outcome: { outcome: 'succeeded' as const },
      })),
    };
    this.semanticOcrJobTargets.delete(active.id);
    this.semanticOcrStatus = {
      ...this.semanticOcrStatus,
      reportedFiles: this.semanticOcrStatus.reportedFiles.filter(
        (reported) => !targets.some((target) => sameSemanticOcrTarget(reported, target)),
      ),
      jobs: this.semanticOcrStatus.jobs.map((job) => (job.id === active.id ? completed : job)),
    };
  }

  advanceSemanticLibraryRevision(): void {
    this.semanticLibrary.revision += 1;
    this.semanticEnrolmentPreviews.clear();
    this.semanticExclusionPlans.clear();
  }

  requireSemanticAvailable(): void {
    if (this.semanticStatus.lifecycle.state === 'unavailable') {
      throw new MockClientError('unavailable', 'Semantic components are unavailable');
    }
  }

  requireSemanticLifecycle(expected: SemanticComponentLifecycle['state']): void {
    this.requireSemanticAvailable();
    if (this.semanticStatus.lifecycle.state !== expected) {
      throw new MockClientError(
        'invalidLifecycle',
        `Expected semantic lifecycle ${expected}, got ${this.semanticStatus.lifecycle.state}`,
      );
    }
  }

  createSemanticMigrationPlan(
    profile: SemanticProfile,
    identity: SemanticModelIdentity,
    estimate: SemanticModelMigrationPlan['estimate'],
  ): SemanticModelMigrationPlan {
    this.semanticMigrationSequence += 1;
    const plan: SemanticModelMigrationPlan = {
      migrationId: `mock-migration-${this.semanticMigrationSequence}`,
      from: this.semanticStatus.activeModel ?? null,
      target: { profile, identity },
      estimate: structuredClone(estimate),
      reason: { reason: 'modelChanged' },
      fullReindex: true,
      requiresConfirmation: true,
      resumable: true,
    };
    this.semanticMigrationPlans.set(plan.migrationId, plan);
    return structuredClone(plan);
  }

  semanticMigrationPlan(migrationId: string): SemanticModelMigrationPlan {
    const plan = this.semanticMigrationPlans.get(migrationId);
    if (plan === undefined) {
      throw new MockClientError('invalidMigrationPlan', 'The migration plan is no longer valid');
    }
    return plan;
  }

  async perform<T>(
    method: MockClientMethod,
    signal: AbortSignal | undefined,
    createValue: () => T,
  ): Promise<T> {
    if (signal?.aborted === true) {
      throw new DOMException('The operation was aborted.', 'AbortError');
    }
    const failure = this.failures[method];
    if (failure !== undefined) {
      throw failure;
    }
    if (this.latencyMs > 0) {
      await this.delay(signal);
    }
    return createValue();
  }

  delay(signal: AbortSignal | undefined): Promise<void> {
    return new Promise((resolve, reject) => {
      const abort = (): void => {
        clearTimeout(timer);
        reject(new DOMException('The operation was aborted.', 'AbortError'));
      };
      const timer = setTimeout(() => {
        signal?.removeEventListener('abort', abort);
        resolve();
      }, this.latencyMs);
      signal?.addEventListener('abort', abort, { once: true });
    });
  }
}
