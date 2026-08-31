import actionFixtures from '../../../../fixtures/mock-responses/actions.json';
import directoryFixtures from '../../../../fixtures/mock-responses/directories.json';
import pluginFixtures from '../../../../fixtures/mock-responses/plugins.json';
import type {
  ActionDescriptor,
  ActionResult,
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ArchiveCredentialRequest,
  BackendEvent,
  BeginOneDriveAuthorizationResponse,
  CalculateFolderSizeRequest,
  CalculateFolderSizeResult,
  ChecksumAlgorithm,
  ChecksumEntry,
  ChecksumFile,
  ChecksumPage,
  ComparisonCriteria,
  ComparisonEntry,
  ComparisonEntrySide,
  ComparisonPage,
  ComparisonStatus,
  Connection,
  ConnectionId,
  CreateConnectionRequest,
  CreateWorkspaceRequest,
  DiagnosticsResult,
  DirectorySnapshot,
  DiscoverApplicationUninstallCandidatesRequest,
  DiscoverApplicationUninstallCandidatesResult,
  DuplicateGroup,
  DuplicatePage,
  EditableFile,
  EditableFileSave,
  EntryMetadata,
  EntryMetadataRequest,
  EntrySummary,
  FileRangeChunk,
  FinderTags,
  GenerateSyncPlanRequest,
  GitFileHistoryRequest,
  GitFileHistoryResult,
  HostKeyProbe,
  InvokeActionRequest,
  ListDirectoryRequest,
  LoadEditableFileRequest,
  Location,
  NavigateRequest,
  OneDriveAuthorizationAttempt,
  OpenStructuredViewRequest,
  Operation,
  OperationId,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  ReadFileRangeRequest,
  ReadStructuredJsonWindowRequest,
  ReadStructuredRowsRequest,
  RemoveApplicationDockIconRequest,
  RemoveApplicationDockIconResult,
  ResolveConflictRequest,
  RuntimeCapabilities,
  SaveChecksumFileRequest,
  SavedChecksumFile,
  SaveEditableFileRequest,
  ScanDiskUsageRequest,
  ScanDiskUsageResult,
  SearchInFileMatch,
  SearchInFileRequest,
  SearchInFileResult,
  SearchQuery,
  SearchStructuredRowsRequest,
  SetPaneActivityRequest,
  Settings,
  SpotlightComment,
  StartChecksumRequest,
  StartChecksumResult,
  StartComparisonRequest,
  StartComparisonResult,
  StartDuplicateScanRequest,
  StartDuplicateScanResult,
  StartOperationRequest,
  StartSearchRequest,
  StartSearchResult,
  StructuredJsonWindow,
  StructuredRowSearch,
  StructuredRows,
  StructuredView,
  StructuredViewSessionRequest,
  StructuredViewStatus,
  SyncPlan,
  SystemLocation,
  Unsubscribe,
  UpdateConnectionRequest,
  UpdateStructuredViewRequest,
  VerificationReport,
  VerificationResult,
  Volume,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import { EventStreamSignalRegistry, MutableEventStreamStatus } from '../events/event-stream';
import type { FileManagerClient, NativeFileDrop } from './file-manager-client';
import {
  createGeneratedDirectory,
  GENERATED_DIRECTORY_SIZES,
  type GeneratedDirectorySize,
} from './mock-directory-generator';

interface FixtureEntry {
  name: string;
  kind: 'file' | 'directory' | 'symlink';
  size?: number;
  hidden?: boolean;
  readable?: boolean;
}

const directories = directoryFixtures as Record<string, FixtureEntry[]>;
const actions = actionFixtures as ActionDescriptor[];
const plugins = pluginFixtures as PluginDescriptor[];

/** Extensions {@link MockFileManagerClient.getThumbnail} fakes a preview for (task 0134). */
const THUMBNAILABLE_MOCK_EXTENSIONS = new Set([
  'jpg',
  'jpeg',
  'png',
  'gif',
  'webp',
  'ico',
  'cbz',
  'cbr',
  'mp4',
  'm4v',
  'mov',
  'pdf',
]);

export type MockClientMethod =
  | 'getRuntimeCapabilities'
  | 'getDiagnostics'
  | 'getSystemLocations'
  | 'getVolumes'
  | 'getHomeDirectory'
  | 'startNativeDrag'
  | 'showPlatformContextMenu'
  | 'getSettings'
  | 'updateSettings'
  | 'getWorkspace'
  | 'listWorkspaces'
  | 'startWorkspace'
  | 'createWorkspace'
  | 'renameWorkspace'
  | 'deleteWorkspace'
  | 'openWorkspace'
  | 'dispatchWorkspaceCommand'
  | 'navigatePane'
  | 'listDirectory'
  | 'listDirectoryChildren'
  | 'getEntryMetadata'
  | 'setPaneActivity'
  | 'getFileIcon'
  | 'getThumbnail'
  | 'getFinderTags'
  | 'setFinderTags'
  | 'getSpotlightComment'
  | 'setSpotlightComment'
  | 'cacheArchivePassword'
  | 'readFileRange'
  | 'searchInFile'
  | 'calculateFolderSize'
  | 'scanDiskUsage'
  | 'cancelDiskUsage'
  | 'discoverApplicationUninstallCandidates'
  | 'removeApplicationDockIcon'
  | 'gitFileHistory'
  | 'startOperation'
  | 'listOperations'
  | 'cancelOperation'
  | 'undoOperation'
  | 'pauseOperation'
  | 'resumeOperation'
  | 'resolveConflict'
  | 'listActions'
  | 'invokeAction'
  | 'listPlugins'
  | 'setPluginEnabled'
  | 'getPluginLogs'
  | 'getPluginIconThemeAsset'
  | 'startSearch'
  | 'cancelSearch'
  | 'startComparison'
  | 'getComparison'
  | 'cancelComparison'
  | 'startChecksums'
  | 'getChecksums'
  | 'cancelChecksums'
  | 'renderChecksumFile'
  | 'saveChecksumFile'
  | 'verifyChecksumFile'
  | 'startDuplicateScan'
  | 'getDuplicateScan'
  | 'cancelDuplicateScan'
  | 'generateSyncPlan'
  | 'applySyncPlan'
  | 'listConnections'
  | 'createConnection'
  | 'getConnection'
  | 'updateConnection'
  | 'deleteConnection'
  | 'connectConnection'
  | 'disconnectConnection'
  | 'testConnection'
  | 'probeSshHostKey'
  | 'acceptSshHostKey'
  | 'beginOneDriveAuthorization'
  | 'getOneDriveAuthorizationAttempt'
  | 'cancelOneDriveAuthorization';

export interface MockFileManagerClientOptions {
  pageSize?: number;
  seed?: number;
  loadingLocations?: readonly string[];
  latencyMs?: number;
  failures?: Partial<Record<MockClientMethod, Error>>;
  nativeIconExtensions?: readonly string[];
}

function fixtureEntry(
  parentUri: string,
  fixture: FixtureEntry,
): import('../../models').EntrySummary {
  const uri = `${parentUri === 'mock:///' ? parentUri : `${parentUri}/`}${encodeURIComponent(fixture.name)}`;
  const extension =
    fixture.kind === 'file' && fixture.name.includes('.')
      ? fixture.name.slice(fixture.name.lastIndexOf('.') + 1)
      : undefined;

  return {
    id: uri,
    location: { providerId: 'file', uri },
    name: fixture.name,
    kind: fixture.kind,
    ...(fixture.size === undefined ? {} : { size: fixture.size }),
    hidden: fixture.hidden ?? false,
    readOnly: fixture.readable === false,
    ...(extension === undefined ? {} : { extension }),
    metadataRevision: 1,
  };
}

/** Sums the byte size and counts the files/symlinks (directories excluded) across a directory's
 * entries, mirroring `fm_application::directory::aggregate_totals` so mock-mode status-bar
 * totals behave like a real backend. */
function aggregateTotals(entries: Iterable<import('../../models').EntrySummary>): {
  size: number;
  fileCount: number;
} {
  let size = 0;
  let fileCount = 0;
  for (const entry of entries) {
    if (entry.kind !== 'directory') {
      size += entry.size ?? 0;
      fileCount += 1;
    }
  }
  return { size, fileCount };
}

/** Matches a filename using the structured predicate, or legacy auto-detected semantics. */
function matchesQuery(name: string, query: string, predicate?: SearchQuery['name']): boolean {
  const mode =
    predicate?.mode ?? (query.includes('*') || query.includes('?') ? 'glob' : 'substring');
  const caseSensitive = predicate?.caseSensitive ?? false;
  const candidate = caseSensitive ? name : name.toLowerCase();
  const pattern = caseSensitive ? query : query.toLowerCase();
  if (mode === 'substring') {
    return candidate.includes(pattern);
  }
  return pattern
    .split(',')
    .map((alternative) => alternative.trim())
    .filter((alternative) => alternative.length > 0)
    .some((alternative) => {
      const escaped = alternative
        .replace(/[.+^${}()|[\]\\]/g, '\\$&')
        .replace(/\*/g, '.*')
        .replace(/\?/g, '.');
      return new RegExp(`^${escaped}$`, 'u').test(candidate);
    });
}

function firstContentMatch(
  text: string,
  query: string,
  predicate?: SearchQuery['content'],
): { index: number; length: number } | undefined {
  if (predicate?.regex) {
    const match = new RegExp(query, predicate.caseSensitive ? 'mu' : 'imu').exec(text);
    return match?.index === undefined
      ? undefined
      : { index: match.index, length: match[0]?.length ?? 0 };
  }
  if (predicate?.wholeWord) {
    const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const match = new RegExp(`\\b${escaped}\\b`, predicate.caseSensitive ? 'mu' : 'imu').exec(text);
    return match?.index === undefined
      ? undefined
      : { index: match.index, length: match[0]?.length ?? 0 };
  }
  const haystack = predicate?.caseSensitive ? text : text.toLowerCase();
  const needle = predicate?.caseSensitive ? query : query.toLowerCase();
  const index = haystack.indexOf(needle);
  return index === -1 ? undefined : { index, length: query.length };
}

/**
 * Deterministically generates plausible multi-line text content for a mock file uri, so the
 * in-app large file viewer (task 0088) has something non-trivial to lazily fetch and search over
 * without real file bytes existing anywhere in the fixture tree.
 */
function syntheticFileContent(uri: string): Uint8Array {
  let seed = 0;
  for (let index = 0; index < uri.length; index += 1) {
    seed = (seed * 31 + uri.charCodeAt(index)) >>> 0;
  }
  const lineCount = 2_000 + (seed % 3_000);
  const lines: string[] = [];
  for (let index = 0; index < lineCount; index += 1) {
    const marker = index % 97 === 0 ? ' ERROR' : '';
    lines.push(`line ${index} of ${uri}${marker}`);
  }
  return new TextEncoder().encode(`${lines.join('\n')}\n`);
}

/**
 * Recursively walks the fixture directory tree from `rootUri` (reduced
 * fidelity vs. the real `fm-search` traversal), silently skipping the
 * `Unreadable` fixture directory the same way `directorySnapshot` treats it.
 *
 * When `contentQuery` is given, only files matching BOTH the filename query and the content
 * query are returned (mirroring the real backend's content-search-with-filename-filter AND
 * semantics), and matching entries get a synthetic `contentMatches` entry pointing at the first
 * match within the file's (deterministic, synthetic) content - see `syntheticFileContent`.
 */
function collectMatches(
  rootUri: string,
  query: string,
  contentQuery: string | undefined,
  showHidden: boolean,
  getContent: (uri: string) => Uint8Array,
  structuredQuery?: SearchQuery,
): import('../../models').EntrySummary[] {
  const results: import('../../models').EntrySummary[] = [];
  const pending = [rootUri];
  while (pending.length > 0) {
    const uri = pending.pop();
    if (uri === undefined || uri === 'mock:///Unreadable') continue;
    const fixtures = directories[uri];
    if (fixtures === undefined) continue;
    for (const fixture of fixtures) {
      const entry = fixtureEntry(uri, fixture);
      if (entry.hidden && !showHidden) continue;
      if (fixture.kind === 'directory') {
        if (structuredQuery?.scope.recurse ?? true) pending.push(entry.location.uri);
      }
      if (
        structuredQuery !== undefined &&
        ((structuredQuery.entryKinds.length > 0 &&
          !structuredQuery.entryKinds.includes(entry.kind)) ||
          (structuredQuery.minSizeBytes !== undefined &&
            (entry.size === undefined || entry.size < structuredQuery.minSizeBytes)) ||
          (structuredQuery.maxSizeBytes !== undefined &&
            (entry.size === undefined || entry.size > structuredQuery.maxSizeBytes)) ||
          (structuredQuery.modifiedAfter !== undefined &&
            (entry.modifiedAt === undefined ||
              entry.modifiedAt.localeCompare(structuredQuery.modifiedAfter) < 0)) ||
          (structuredQuery.modifiedBefore !== undefined &&
            (entry.modifiedAt === undefined ||
              entry.modifiedAt.localeCompare(structuredQuery.modifiedBefore) > 0)) ||
          (structuredQuery.mimeTypes.length > 0 &&
            (entry.mimeType === undefined ||
              !structuredQuery.mimeTypes.some((mime) =>
                mime.endsWith('/*')
                  ? entry.mimeType?.startsWith(mime.slice(0, -1))
                  : entry.mimeType === mime,
              ))))
      ) {
        continue;
      }
      if (contentQuery !== undefined && contentQuery !== '') {
        if (fixture.kind !== 'file' || !matchesQuery(fixture.name, query, structuredQuery?.name)) {
          continue;
        }
        const text = new TextDecoder().decode(getContent(entry.location.uri));
        const match = firstContentMatch(text, contentQuery, structuredQuery?.content);
        if (match === undefined) continue;
        const lineNumber = text.slice(0, match.index).split('\n').length;
        results.push({
          ...entry,
          contentMatches: [{ lineNumber, offset: match.index, length: match.length }],
        });
        continue;
      }
      if (matchesQuery(fixture.name, query, structuredQuery?.name)) {
        results.push(entry);
      }
    }
  }
  return results;
}

/**
 * Recursively walks a fixture subtree from `rootUri`, keyed by path relative
 * to that root (task 0075). Reduced fidelity vs. the real `fm-comparison`
 * traversal, same trade-off `collectMatches` above documents for search.
 */
function walkFixtureTree(rootUri: string, showHidden: boolean): Map<string, EntrySummary> {
  const result = new Map<string, EntrySummary>();
  const pending: { uri: string; relativePath: string }[] = [{ uri: rootUri, relativePath: '' }];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined || current.uri === 'mock:///Unreadable') continue;
    const fixtures = directories[current.uri];
    if (fixtures === undefined) continue;
    for (const fixture of fixtures) {
      const entry = fixtureEntry(current.uri, fixture);
      if (entry.hidden && !showHidden) continue;
      const relativePath =
        current.relativePath === '' ? fixture.name : `${current.relativePath}/${fixture.name}`;
      result.set(relativePath, entry);
      if (fixture.kind === 'directory') {
        pending.push({ uri: entry.location.uri, relativePath });
      }
    }
  }
  return result;
}

function comparisonEntrySideFor(entry: EntrySummary): ComparisonEntrySide {
  return {
    kind: entry.kind,
    ...(entry.size === undefined ? {} : { size: entry.size }),
  };
}

/** The last `/`-separated segment of a URI, percent-decoded. */
function lastSegment(uri: string): string {
  const trimmed = uri.endsWith('/') ? uri.slice(0, -1) : uri;
  const index = trimmed.lastIndexOf('/');
  return decodeURIComponent(index === -1 ? trimmed : trimmed.slice(index + 1));
}

/**
 * A deterministic, plausible-looking digest for the mock runtime.
 *
 * Deliberately *not* a real hash: the mock never reads file bytes, and a
 * digest that merely looks right is enough to exercise the UI. It is stable
 * for a given (uri, algorithm) pair so repeated runs and the verify flow
 * agree with themselves.
 */
function mockDigest(uri: string, algorithm: ChecksumAlgorithm): string {
  const width = algorithm === 'crc32' ? 8 : algorithm === 'md5' ? 32 : 64;
  let hash = 0x811c9dc5;
  const seed = `${algorithm}:${uri}`;
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  let digest = '';
  let state = hash;
  while (digest.length < width) {
    state = (Math.imul(state, 0x01000193) ^ digest.length) >>> 0;
    digest += state.toString(16).padStart(8, '0');
  }
  return digest.slice(0, width);
}

/**
 * Builds a plausible duplicate-scan result: one group of two byte-identical
 * files with distinct inodes, plus one hardlink cluster, so the review UI can
 * exercise both categories without a real filesystem.
 */
function buildMockDuplicateGroups(roots: readonly Location[]): DuplicateGroup[] {
  const root = roots[0];
  if (root === undefined) return [];
  const base = root.uri.endsWith('/') ? root.uri.slice(0, -1) : root.uri;
  const at = (name: string): Location => ({ providerId: root.providerId, uri: `${base}/${name}` });
  return [
    {
      fullHash: mockDigest(`${base}/duplicate-content`, 'sha256'),
      size: 20_480,
      hardlinkClusters: [],
      distinctLocations: [at('report-copy.pdf'), at('archive/report.pdf')],
      reclaimableBytes: 20_480,
    },
    {
      fullHash: mockDigest(`${base}/hardlinked-content`, 'sha256'),
      size: 4_096,
      hardlinkClusters: [
        {
          device: 16_777_233,
          inode: 4_242_424,
          locations: [at('notes.md'), at('archive/notes-link.md')],
        },
      ],
      distinctLocations: [],
      // A hardlink cluster is one file: deleting a path frees nothing.
      reclaimableBytes: 0,
    },
  ];
}

/**
 * Builds a plausible comparison between two fixture subtrees. Directories
 * are always reported identical (matching the real engine's rule that a
 * matched directory pair defers entirely to its children); matched files
 * compare by size once `criteria` is not `nameOnly`.
 */
function buildMockComparisonEntries(
  leftRootUri: string,
  rightRootUri: string,
  criteria: ComparisonCriteria,
  showHidden: boolean,
): ComparisonEntry[] {
  const left = walkFixtureTree(leftRootUri, showHidden);
  const right = walkFixtureTree(rightRootUri, showHidden);
  const relativePaths = [...new Set([...left.keys(), ...right.keys()])].sort();
  return relativePaths.map((relativePath) => {
    const leftEntry = left.get(relativePath);
    const rightEntry = right.get(relativePath);
    let status: ComparisonStatus;
    if (leftEntry === undefined) {
      status = 'onlyRight';
    } else if (rightEntry === undefined) {
      status = 'onlyLeft';
    } else if (leftEntry.kind !== rightEntry.kind) {
      status = 'typeMismatch';
    } else if (leftEntry.kind === 'directory' || criteria === 'nameOnly') {
      status = 'identical';
    } else {
      status = (leftEntry.size ?? 0) === (rightEntry.size ?? 0) ? 'identical' : 'differentSize';
    }
    return {
      relativePath,
      ...(leftEntry === undefined ? {} : { left: comparisonEntrySideFor(leftEntry) }),
      ...(rightEntry === undefined ? {} : { right: comparisonEntrySideFor(rightEntry) }),
      status,
    };
  });
}

/** Mirrors `fm_comparison::sync::default_action`'s per-status proposal rules. */
function defaultSyncAction(
  status: ComparisonStatus,
  mode: GenerateSyncPlanRequest['mode'],
): SyncPlan['items'][number]['action'] {
  if (mode === 'mirrorLeftToRight') {
    if (status === 'onlyRight') return 'deleteRight';
    if (
      status === 'onlyLeft' ||
      status === 'newer' ||
      status === 'older' ||
      status === 'differentSize'
    )
      return 'copyLeftToRight';
    return 'skip';
  }
  if (mode === 'mirrorRightToLeft') {
    if (status === 'onlyLeft') return 'deleteLeft';
    if (
      status === 'onlyRight' ||
      status === 'newer' ||
      status === 'older' ||
      status === 'differentSize'
    )
      return 'copyRightToLeft';
    return 'skip';
  }
  // twoWayUpdate
  if (status === 'onlyLeft' || status === 'newer') return 'copyLeftToRight';
  if (status === 'onlyRight' || status === 'older') return 'copyRightToLeft';
  return 'skip';
}

function createMockWorkspace(id: WorkspaceId, name = 'Mock Workspace'): WorkspaceProjection {
  return {
    id,
    name,
    revision: 1,
    paneOrder: ['left', 'right'],
    panesById: {
      left: {
        id: 'left',
        tabOrder: ['left-tab'],
        tabsById: {
          'left-tab': {
            id: 'left-tab',
            title: 'Mock files',
            location: { providerId: 'file', uri: 'mock:///' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: {
              sort: [],
              columns: [],
              showHidden: false,
              foldersFirst: true,
              quickFilter: null,
            },
          },
        },
        activeTabId: 'left-tab',
      },
      right: {
        id: 'right',
        tabOrder: ['right-tab'],
        tabsById: {
          'right-tab': {
            id: 'right-tab',
            title: 'Documents',
            location: { providerId: 'file', uri: 'mock:///Documents' },
            canNavigateBack: false,
            canNavigateForward: false,
            view: {
              sort: [],
              columns: [],
              showHidden: false,
              foldersFirst: true,
              quickFilter: null,
            },
          },
        },
        activeTabId: 'right-tab',
      },
    },
    activePaneId: 'left',
    layout: {
      type: 'split',
      axis: 'horizontal',
      ratio: 0.5,
      first: { type: 'pane', paneId: 'left' },
      second: { type: 'pane', paneId: 'right' },
    },
    operationCentre: { visible: false, height: 180 },
    ephemeral: false,
  };
}

/**
 * Mirrors the backend's honest, pre-0104/0106 `connect`/`test` scope (see
 * `fm_connections::ConnectionService`'s documentation): with no real
 * protocol dialer, a connection is "usable" once its typed configuration is
 * well-formed and, for an SSH configuration whose authentication method
 * needs one, a credential is stored.
 */
function evaluateMockConnectionStatus(connection: Connection): Connection['status'] {
  if (connection.configuration.kind === 'ssh') {
    const needsStoredCredential =
      connection.configuration.authentication === 'password' ||
      connection.configuration.authentication === 'privateKey';
    if (needsStoredCredential && !connection.hasCredential) {
      return 'authenticationRequired';
    }
  }
  return 'connected';
}

/** Strictly typed controls for the deterministic in-memory frontend adapter. */
export class MockFileManagerClient implements FileManagerClient {
  readonly connection = new MutableEventStreamStatus();

  cacheArchivePassword(_request: ArchiveCredentialRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('cacheArchivePassword', signal, () => undefined);
  }
  private readonly resynchronise = new EventStreamSignalRegistry();
  private readonly pageSize: number;
  private readonly seed: number;
  private readonly loadingLocations: ReadonlySet<string>;
  private readonly latencyMs: number;
  private readonly failures: Partial<Record<MockClientMethod, Error>>;
  private readonly nativeIconExtensions: ReadonlySet<string>;
  private readonly finderTagsByUri = new Map<string, FinderTags>();
  private readonly spotlightCommentsByUri = new Map<string, SpotlightComment>();
  private readonly listeners = new Set<(event: BackendEvent) => void>();
  private readonly scriptedEvents: BackendEvent[] = [];
  private readonly operations = new Map<OperationId, Operation>();
  private readonly navigationHistory = new Map<string, { back: Location[]; forward: Location[] }>();
  private readonly workspaces = new Map<WorkspaceId, WorkspaceProjection>();
  private readonly connections = new Map<ConnectionId, Connection>();
  private readonly oneDriveAuthorizations = new Map<
    string,
    { readonly connectionId: ConnectionId; attempt: OneDriveAuthorizationAttempt }
  >();
  private pluginState: PluginDescriptor[] = structuredClone(plugins);
  private settings: Settings = {
    schemaVersion: 5,
    theme: 'auto',
    language: 'en',
    fontSize: 13,
    rowHeight: 20,
    dateFormat: 'medium',
    sizeFormat: 'binary',
    showHiddenFiles: false,
    confirmPermanentDelete: true,
    defaultConflictPolicy: 'ask',
    operationConcurrency: 2,
    defaultPaneLayout: 'dual',
    defaultColumns: ['core.name', 'core.extension', 'core.size', 'core.modified'],
    columnWidths: {},
    keybindings: {},
    enabledPlugins: [],
    pluginSettings: {},
    terminalCommand: null,
    editorCommand: null,
    defaultStartLocations: [],
    favouriteLocations: [],
    recentLocationsByWorkspace: {},
    multiRenamePresets: [],
    savedSearches: [],
    iconTheme: 'generic',
  };
  private operationSequence = 0;
  private tabSequence = 0;
  private workspaceSequence = 0;
  private connectionSequence = 0;
  private oneDriveAuthorizationSequence = 0;
  private searchSequence = 0;
  private eventSequence = 0;
  private readonly searches = new Map<
    string,
    { cancelled: boolean; entries: readonly EntrySummary[] }
  >();
  private comparisonSequence = 0;
  private readonly comparisons = new Map<
    string,
    {
      cancelled: boolean;
      entries: readonly ComparisonEntry[];
      left: Location;
      right: Location;
      criteria: ComparisonCriteria;
    }
  >();
  private checksumSequence = 0;
  private readonly checksumJobs = new Map<
    string,
    {
      cancelled: boolean;
      entries: readonly ChecksumEntry[];
      algorithms: readonly ChecksumAlgorithm[];
    }
  >();
  private duplicateScanSequence = 0;
  private readonly duplicateScans = new Map<
    string,
    { cancelled: boolean; groups: readonly DuplicateGroup[]; roots: readonly Location[] }
  >();
  private readonly fileContents = new Map<string, Uint8Array>();
  private readonly structuredSessions = new Map<
    string,
    {
      uri: string;
      format: OpenStructuredViewRequest['format'];
      delimiter: string;
      headerMode: NonNullable<OpenStructuredViewRequest['headerMode']>;
    }
  >();
  // Generated directories are recreated per request, but their aggregate totals are a pure
  // function of (size, seed) — cache them instead of resumming up to 1,000,000 entries on every
  // paginated fetch.
  private readonly generatedTotalsCache = new Map<
    GeneratedDirectorySize,
    { readonly size: number; readonly fileCount: number }
  >();

  constructor(options: MockFileManagerClientOptions = {}) {
    this.pageSize = options.pageSize ?? 100;
    this.seed = options.seed ?? 13;
    this.loadingLocations = new Set(options.loadingLocations);
    this.latencyMs = options.latencyMs ?? 0;
    this.failures = options.failures ?? {};
    this.nativeIconExtensions = new Set(
      options.nativeIconExtensions?.map((extension) => extension.toLowerCase()),
    );
  }

  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities> {
    return this.perform('getRuntimeCapabilities', signal, () => ({
      clipboard: false,
      extendedAttributes: true,
      finderTags: true,
      nativeDragOut: false,
      nativeFileIcons: this.nativeIconExtensions.size > 0,
      nativeMenus: false,
      platformContextMenu: false,
      nativeThumbnails: false,
      openTerminal: false,
      platform: 'linux',
      plugins: true,
      revealInSystemFileManager: false,
      runtime: 'mock',
      serverAdministration: false,
      systemTrash: false,
    }));
  }

  getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]> {
    return this.perform('getSystemLocations', signal, () => []);
  }

  getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult> {
    return this.perform('getDiagnostics', signal, () => ({
      frontendVersion: '0.1.0',
      backendVersion: '0.1.0',
      platform: 'Mock',
      runtimeCapabilities: {
        clipboard: false,
        extendedAttributes: true,
        finderTags: true,
        nativeDragOut: false,
        nativeFileIcons: this.nativeIconExtensions.size > 0,
        nativeMenus: false,
        platformContextMenu: false,
        nativeThumbnails: false,
        openTerminal: false,
        platform: 'linux',
        plugins: true,
        revealInSystemFileManager: false,
        runtime: 'mock',
        serverAdministration: false,
        systemTrash: false,
      },
      connectionState: {
        connected: true,
        uptimeSeconds: 0,
        eventsReceived: 0,
        statusMessage: 'Mock',
      },
      loadedPlugins: [],
      recentErrors: [],
      operationQueueStatus: {
        queuedCount: 0,
        runningCount: 0,
        pausedCount: 0,
        completedCount: 0,
        totalPendingSize: 0,
      },
    }));
  }

  getVolumes(signal?: AbortSignal): Promise<Volume[]> {
    return this.perform('getVolumes', signal, () => [
      { name: 'Macintosh HD', location: { providerId: 'file', uri: 'mock:///' } },
      { name: 'Empty Drive', location: { providerId: 'file', uri: 'mock:///Empty' } },
    ]);
  }

  getHomeDirectory(signal?: AbortSignal): Promise<string | undefined> {
    return this.perform('getHomeDirectory', signal, () => '/Users/mock');
  }

  startNativeDrag(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.perform('startNativeDrag', signal, () => undefined);
  }

  showPlatformContextMenu(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.perform('showPlatformContextMenu', signal, () => undefined);
  }

  subscribeNativeFileDrops(_listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe> {
    return Promise.resolve(() => undefined);
  }

  getSettings(signal?: AbortSignal): Promise<Settings> {
    return this.perform('getSettings', signal, () => structuredClone(this.settings));
  }

  getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined> {
    return this.perform('getFileIcon', signal, () => {
      const pathname = new URL(sampleLocationUri).pathname;
      const name = pathname.slice(pathname.lastIndexOf('/') + 1);
      const extension = name.includes('.')
        ? name.slice(name.lastIndexOf('.') + 1).toLowerCase()
        : '';
      if (!this.nativeIconExtensions.has(extension)) return undefined;
      return new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    });
  }

  getThumbnail(
    locationUri: string,
    _size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    return this.perform('getThumbnail', signal, () => {
      const pathname = new URL(locationUri).pathname;
      const name = pathname.slice(pathname.lastIndexOf('/') + 1);
      const extension = name.includes('.')
        ? name.slice(name.lastIndexOf('.') + 1).toLowerCase()
        : '';
      if (!THUMBNAILABLE_MOCK_EXTENSIONS.has(extension)) return undefined;
      // JPEG magic bytes - just needs to look like an image, not decode as one.
      return new Uint8Array([0xff, 0xd8, 0xff, 0xe0]);
    });
  }

  getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined> {
    return this.perform(
      'getFinderTags',
      signal,
      () => this.finderTagsByUri.get(locationUri) ?? { tags: [] },
    );
  }

  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags> {
    return this.perform('setFinderTags', signal, () => {
      const persisted = structuredClone(tags);
      this.finderTagsByUri.set(locationUri, persisted);
      return structuredClone(persisted);
    });
  }

  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined> {
    return this.perform(
      'getSpotlightComment',
      signal,
      () => this.spotlightCommentsByUri.get(locationUri) ?? { comment: null },
    );
  }

  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment> {
    return this.perform('setSpotlightComment', signal, () => {
      const persisted = structuredClone(comment);
      this.spotlightCommentsByUri.set(locationUri, persisted);
      return structuredClone(persisted);
    });
  }

  updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings> {
    return this.perform('updateSettings', signal, () => {
      this.settings = structuredClone(settings);
      return structuredClone(this.settings);
    });
  }

  listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]> {
    return this.perform('listWorkspaces', signal, () =>
      [...this.workspaces.values()].map(({ id, name, revision, ephemeral }) => ({
        id,
        name,
        revision,
        ephemeral,
        updatedAt: '2026-01-01T00:00:00.000Z',
      })),
    );
  }

  createWorkspace(
    request: CreateWorkspaceRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.perform('createWorkspace', signal, () => {
      this.workspaceSequence += 1;
      const workspace = createMockWorkspace(
        `mock-workspace-${this.workspaceSequence}`,
        request.name ?? 'Default',
      );
      this.workspaces.set(workspace.id, workspace);
      return structuredClone(workspace);
    });
  }

  startWorkspace(workspaceId?: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.perform('startWorkspace', signal, () => {
      const existing = workspaceId === undefined ? undefined : this.workspaces.get(workspaceId);
      if (existing !== undefined) return structuredClone(existing);
      const [first] = this.workspaces.values();
      if (first !== undefined) return structuredClone(first);
      this.workspaceSequence += 1;
      const workspace = createMockWorkspace(`mock-workspace-${this.workspaceSequence}`, 'Default');
      this.workspaces.set(workspace.id, workspace);
      return structuredClone(workspace);
    });
  }

  getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.perform('getWorkspace', signal, () => {
      const workspace = this.workspaces.get(workspaceId) ?? createMockWorkspace(workspaceId);
      this.workspaces.set(workspaceId, workspace);
      return structuredClone(workspace);
    });
  }

  renameWorkspace(
    workspaceId: WorkspaceId,
    name: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.dispatchWorkspaceCommand(
      { type: 'renameWorkspace', workspaceId, name, expectedRevision },
      signal,
    );
  }

  deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.perform('deleteWorkspace', signal, () => {
      const workspace = this.workspaces.get(workspaceId);
      if (workspace !== undefined && expectedRevision !== undefined) {
        this.requireWorkspaceRevision(workspace, expectedRevision);
      }
      this.workspaces.delete(workspaceId);
    });
  }

  openWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.perform('openWorkspace', signal, () => {
      const workspace = this.workspaces.get(workspaceId) ?? createMockWorkspace(workspaceId);
      this.workspaces.set(workspaceId, workspace);
      return structuredClone(workspace);
    });
  }

  dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.perform('dispatchWorkspaceCommand', signal, () => {
      const current =
        this.workspaces.get(command.workspaceId) ?? createMockWorkspace(command.workspaceId);
      this.requireWorkspaceRevision(current, command.expectedRevision);
      let changed: WorkspaceProjection;
      switch (command.type) {
        case 'renameWorkspace':
          changed = { ...current, name: command.name, revision: current.revision + 1 };
          break;
        case 'setActivePane':
          changed = {
            ...current,
            activePaneId: command.paneId,
            revision: current.revision + 1,
          };
          break;
        case 'updateLayout':
          changed = { ...current, layout: command.layout, revision: current.revision + 1 };
          break;
        case 'addTab':
        case 'addTransientTab': {
          const pane = current.panesById[command.paneId];
          if (pane === undefined) {
            throw new MockClientError('paneNotFound', `No mock pane with id ${command.paneId}`);
          }
          this.tabSequence += 1;
          const tabId = `mock-tab-${this.tabSequence}`;
          const tab = {
            id: tabId,
            title: command.location.uri.split('/').at(-1) || command.location.uri,
            location: command.location,
            canNavigateBack: false,
            canNavigateForward: false,
            view: {
              sort: [],
              columns: [],
              showHidden: false,
              foldersFirst: true,
              quickFilter: null,
            },
          };
          changed = {
            ...current,
            revision: current.revision + 1,
            panesById: {
              ...current.panesById,
              [pane.id]: {
                ...pane,
                tabOrder: [...pane.tabOrder, tabId],
                tabsById: { ...pane.tabsById, [tabId]: tab },
                activeTabId: tabId,
              },
            },
          };
          break;
        }
        case 'moveTab': {
          const source = current.panesById[command.sourcePaneId];
          const target = current.panesById[command.targetPaneId];
          const tab = source?.tabsById[command.tabId];
          if (source === undefined || target === undefined) {
            throw new MockClientError('paneNotFound', 'No mock source or target pane');
          }
          if (tab === undefined) {
            throw new MockClientError('tabNotFound', `No mock tab with id ${command.tabId}`);
          }
          const sourceOrder = source.tabOrder.filter((tabId) => tabId !== command.tabId);
          const sourceTabs = { ...source.tabsById };
          delete sourceTabs[command.tabId];
          let sourceActiveTabId = source.activeTabId;
          if (source.id !== target.id && sourceOrder.length === 0) {
            this.tabSequence += 1;
            const replacementId = `mock-tab-${this.tabSequence}`;
            sourceOrder.push(replacementId);
            sourceTabs[replacementId] = {
              id: replacementId,
              title: 'Mock files',
              location: { providerId: 'file', uri: 'mock:///' },
              canNavigateBack: false,
              canNavigateForward: false,
              view: {
                sort: [],
                columns: [],
                showHidden: false,
                foldersFirst: true,
                quickFilter: null,
              },
            };
            sourceActiveTabId = replacementId;
          } else if (source.id !== target.id && source.activeTabId === command.tabId) {
            sourceActiveTabId = sourceOrder[0] ?? source.activeTabId;
          }
          const targetOrder =
            source.id === target.id
              ? sourceOrder
              : target.tabOrder.filter((tabId) => tabId !== command.tabId);
          targetOrder.splice(Math.min(command.targetIndex, targetOrder.length), 0, command.tabId);
          changed = {
            ...current,
            activePaneId: source.id === target.id ? current.activePaneId : target.id,
            revision: current.revision + 1,
            panesById: {
              ...current.panesById,
              [source.id]: {
                ...source,
                tabOrder: source.id === target.id ? targetOrder : sourceOrder,
                tabsById: source.id === target.id ? source.tabsById : sourceTabs,
                activeTabId: sourceActiveTabId,
              },
              ...(source.id === target.id
                ? {}
                : {
                    [target.id]: {
                      ...target,
                      tabOrder: targetOrder,
                      tabsById: { ...target.tabsById, [command.tabId]: tab },
                      activeTabId: command.tabId,
                    },
                  }),
            },
          };
          break;
        }
        case 'closeTab':
        case 'activateTab':
        case 'navigateTab':
        case 'updateView': {
          const pane = current.panesById[command.paneId];
          if (pane === undefined) {
            throw new MockClientError('paneNotFound', `No mock pane with id ${command.paneId}`);
          }
          if (command.type === 'closeTab') {
            const tabsById = { ...pane.tabsById };
            delete tabsById[command.tabId];
            const tabOrder = pane.tabOrder.filter((tabId) => tabId !== command.tabId);
            changed = {
              ...current,
              revision: current.revision + 1,
              panesById: {
                ...current.panesById,
                [pane.id]: {
                  ...pane,
                  tabOrder,
                  tabsById,
                  activeTabId: tabOrder[0] ?? pane.activeTabId,
                },
              },
            };
            break;
          }
          const tab = pane.tabsById[command.tabId];
          if (tab === undefined) {
            throw new MockClientError('tabNotFound', `No mock tab with id ${command.tabId}`);
          }
          const historyKey = `${current.id}:${pane.id}:${tab.id}`;
          const history = this.navigationHistory.get(historyKey) ?? { back: [], forward: [] };
          let navigatedLocation = tab.location;
          if (command.type === 'navigateTab') {
            if (command.navigationMode === 'push' && command.location != null) {
              if (command.location.uri !== tab.location.uri) {
                history.back.push(tab.location);
              }
              history.forward = [];
              navigatedLocation = command.location;
            } else if (command.navigationMode === 'back') {
              const target = history.back.pop();
              if (target !== undefined) {
                history.forward.push(tab.location);
                navigatedLocation = target;
              }
            } else if (command.navigationMode === 'forward') {
              const target = history.forward.pop();
              if (target !== undefined) {
                history.back.push(tab.location);
                navigatedLocation = target;
              }
            } else if (command.navigationMode === 'refresh' && command.location != null) {
              navigatedLocation = command.location;
            }
            this.navigationHistory.set(historyKey, history);
          }
          const nextTab =
            command.type === 'navigateTab'
              ? {
                  ...tab,
                  location: navigatedLocation,
                  canNavigateBack: history.back.length > 0,
                  canNavigateForward: history.forward.length > 0,
                }
              : command.type === 'updateView'
                ? {
                    ...tab,
                    view: {
                      ...tab.view,
                      ...Object.fromEntries(
                        Object.entries(command.patch).filter(
                          ([key, value]) => key !== 'quickFilter' && value !== null,
                        ),
                      ),
                      ...(command.patch.quickFilter === undefined
                        ? {}
                        : {
                            quickFilter:
                              command.patch.quickFilter === null ||
                              command.patch.quickFilter.type === 'clear'
                                ? null
                                : command.patch.quickFilter.filter,
                          }),
                    },
                  }
                : tab;
          changed = {
            ...current,
            revision: current.revision + 1,
            panesById: {
              ...current.panesById,
              [pane.id]: {
                ...pane,
                activeTabId: command.type === 'activateTab' ? command.tabId : pane.activeTabId,
                tabsById: { ...pane.tabsById, [tab.id]: nextTab },
              },
            },
          };
          break;
        }
      }
      this.workspaces.set(changed.id, changed);
      return structuredClone(changed);
    });
  }

  private requireWorkspaceRevision(workspace: WorkspaceProjection, expectedRevision: number): void {
    if (workspace.revision !== expectedRevision) {
      throw new MockClientError(
        'workspaceRevisionConflict',
        'The workspace changed after this view was loaded.',
      );
    }
  }

  navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.directorySnapshot(request, signal, 'navigatePane');
  }

  listDirectory(request: ListDirectoryRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.directorySnapshot(request, signal, 'listDirectory');
  }

  listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]> {
    return this.perform('listDirectoryChildren', signal, () => {
      const fixtures = directories[location.uri] ?? [];
      return fixtures
        .map((fixture) => fixtureEntry(location.uri, fixture))
        .filter((entry) => entry.kind === 'directory' && (showHidden || !entry.hidden));
    });
  }

  getEntryMetadata(request: EntryMetadataRequest, signal?: AbortSignal): Promise<EntryMetadata> {
    return this.perform('getEntryMetadata', signal, () => ({
      entryId: request.entryId,
      permissions: { readable: true, writable: true, executable: false },
      ownership: { owner: 'mock-user', group: 'mock-group' },
      extendedAttributes: {},
      checksums: {},
      pluginFields: {},
    }));
  }

  setPaneActivity(_request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('setPaneActivity', signal, () => undefined);
  }

  readFileRange(request: ReadFileRangeRequest, signal?: AbortSignal): Promise<FileRangeChunk> {
    return this.perform('readFileRange', signal, () => {
      if (request.length <= 0) {
        throw new MockClientError('invalidRequest', 'length must be a positive number of bytes');
      }
      const bytes = this.fileContentFor(request.location.uri);
      const end = Math.min(bytes.length, request.offset + request.length);
      const slice = bytes.slice(request.offset, Math.max(request.offset, end));
      return {
        data: Array.from(slice),
        offset: request.offset,
        length: slice.length,
        eof: end >= bytes.length,
        ...(request.offset === 0 ? { probablyBinary: false } : {}),
      };
    });
  }

  openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    return this.perform('readFileRange', signal, () => {
      const sessionId = crypto.randomUUID();
      const delimiter = request.delimiter ?? (request.format === 'tsv' ? '\t' : ',');
      this.structuredSessions.set(sessionId, {
        uri: request.location.uri,
        format: request.format,
        delimiter,
        headerMode: request.headerMode ?? 'auto',
      });
      return this.mockStructuredView(sessionId);
    });
  }

  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus> {
    return this.perform('readFileRange', signal, () => {
      const view = this.mockStructuredView(request.sessionId);
      return {
        indexedBytes: view.sourceBytes,
        indexedRows: view.indexedRows,
        totalRows: view.totalRows ?? null,
        indexingComplete: true,
      };
    });
  }

  updateStructuredView(
    request: UpdateStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    return this.perform('readFileRange', signal, () => {
      const session = this.structuredSession(request.sessionId);
      if (request.delimiter != null) session.delimiter = request.delimiter;
      if (request.headerMode != null) session.headerMode = request.headerMode;
      return this.mockStructuredView(request.sessionId);
    });
  }

  readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows> {
    return this.perform('readFileRange', signal, () => {
      const view = this.mockStructuredView(request.sessionId);
      return {
        rows: view.rows.slice(request.startRow, request.startRow + request.count),
        indexedRows: view.indexedRows,
        totalRows: view.totalRows ?? null,
        indexingComplete: true,
      };
    });
  }

  readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    signal?: AbortSignal,
  ): Promise<StructuredJsonWindow> {
    return this.perform('readFileRange', signal, () => {
      const session = this.structuredSession(request.sessionId);
      const bytes = this.fileContentFor(session.uri);
      const data = bytes.slice(request.offset, request.offset + request.length);
      return {
        data: Array.from(data),
        offset: request.offset,
        eof: request.offset + data.length >= bytes.length,
        tokens: [],
      };
    });
  }

  searchStructuredRows(
    request: SearchStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRowSearch> {
    return this.perform('readFileRange', signal, () => {
      const rows = this.mockStructuredView(request.sessionId).rows;
      const matches = rows
        .slice(request.cursor)
        .filter((row) =>
          row.cells.some((cell) => cell.toLowerCase().includes(request.query.toLowerCase())),
        )
        .slice(0, request.limit);
      const last = matches.at(-1)?.index;
      return {
        matches,
        nextCursor: last === undefined || last + 1 >= rows.length ? null : last + 1,
        indexingComplete: true,
      };
    });
  }

  closeStructuredView(request: StructuredViewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('readFileRange', signal, () => {
      if (!this.structuredSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'structured viewer session not found');
      }
    });
  }

  loadEditableFile(request: LoadEditableFileRequest, signal?: AbortSignal): Promise<EditableFile> {
    return this.perform('readFileRange', signal, () => {
      const bytes = this.fileContentFor(request.location.uri);
      return {
        content: new TextDecoder().decode(bytes),
        revision: String(bytes.length),
        size: bytes.length,
      };
    });
  }

  saveEditableFile(
    request: SaveEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFileSave> {
    return this.perform('readFileRange', signal, () => {
      const existing = this.fileContentFor(request.location.uri);
      if (!request.overwriteConflict && request.expectedRevision !== String(existing.length)) {
        throw new MockClientError('fileRevisionConflict', 'The file changed after it was loaded.');
      }
      const bytes = new TextEncoder().encode(request.content);
      this.fileContents.set(request.destination?.uri ?? request.location.uri, bytes);
      return {
        revision: String(bytes.length),
        size: bytes.length,
        overwroteConflict: request.overwriteConflict,
      };
    });
  }

  searchInFile(request: SearchInFileRequest, signal?: AbortSignal): Promise<SearchInFileResult> {
    return this.perform('searchInFile', signal, () => {
      if (request.query.length === 0) {
        throw new MockClientError('invalidRequest', 'search query must not be empty');
      }
      const matchesOnLine = this.buildLineMatcher(request);
      const text = new TextDecoder().decode(this.fileContentFor(request.location.uri));
      const lines = text.split('\n');
      const matches: SearchInFileMatch[] = [];
      let truncated = false;
      let fileOffset = 0;
      for (let lineIndex = 0; lineIndex < lines.length && !truncated; lineIndex += 1) {
        const line = lines[lineIndex] ?? '';
        for (const [start, end] of matchesOnLine(line)) {
          if (matches.length >= 5_000) {
            truncated = true;
            break;
          }
          matches.push({
            lineNumber: lineIndex + 1,
            offset: fileOffset + start,
            length: end - start,
          });
        }
        fileOffset += line.length + 1;
      }
      return { matches, truncated };
    });
  }

  calculateFolderSize(
    request: CalculateFolderSizeRequest,
    signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult> {
    return this.perform('calculateFolderSize', signal, () => {
      if (directories[request.location.uri] === undefined) {
        throw new MockClientError(
          'directoryNotFound',
          `No mock directory at ${request.location.uri}`,
        );
      }
      let totalBytes = 0;
      let fileCount = 0;
      const stack = [request.location.uri];
      while (stack.length > 0) {
        const uri = stack.pop() as string;
        for (const fixture of directories[uri] ?? []) {
          const entry = fixtureEntry(uri, fixture);
          if (entry.kind === 'directory') {
            stack.push(entry.location.uri);
          } else {
            totalBytes += entry.size ?? 0;
            fileCount += 1;
          }
        }
      }
      return { totalBytes, fileCount };
    });
  }

  scanDiskUsage(request: ScanDiskUsageRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('scanDiskUsage', signal, () => {
      const fixtures = directories[request.location.uri];
      if (fixtures === undefined) {
        throw new MockClientError(
          'directoryNotFound',
          `No mock directory at ${request.location.uri}`,
        );
      }
      const collapsedNames = new Set(['.git', '.hg', '.svn', 'node_modules']);
      const build = (uri: string, name: string, isRoot = false): ScanDiskUsageResult['root'] => {
        const children = (directories[uri] ?? []).map((fixture) => {
          const entry = fixtureEntry(uri, fixture);
          if (entry.kind === 'directory') return build(entry.location.uri, entry.name);
          return {
            name: entry.name,
            location: entry.location,
            kind: entry.kind,
            logicalBytes: entry.size ?? 0,
            physicalBytes: entry.size ?? 0,
            collapsed: false,
            children: [],
          };
        });
        const physicalBytes = children.reduce((sum, child) => sum + child.physicalBytes, 0);
        const collapsed = collapsedNames.has(name) && !(isRoot && request.expandRoot === true);
        return {
          name,
          location: { providerId: request.location.providerId, uri },
          kind: 'directory',
          logicalBytes: physicalBytes,
          physicalBytes,
          collapsed,
          children: collapsed ? [] : children,
        };
      };
      const name = decodeURIComponent(
        request.location.uri.replace(/\/+$/u, '').split('/').at(-1) ?? '/',
      );
      const result = {
        root: build(request.location.uri, name, true),
        unreadableEntries: 0,
        unreadable: [],
        scannedEntries: 1,
      };
      const first = result.root.children.at(0);
      if (first !== undefined) {
        this.eventSequence += 1;
        this.emit({
          eventId: this.eventSequence,
          timestamp: new Date().toISOString(),
          workspaceId: request.workspaceId as WorkspaceId,
          payload: {
            type: 'diskUsage.progress',
            scanId: request.scanId,
            root: {
              ...result.root,
              logicalBytes: first.logicalBytes,
              physicalBytes: first.physicalBytes,
              children: [first],
            },
            unreadableEntries: 0,
            unreadable: [],
            scannedEntries: 1,
            isComplete: false,
          },
        });
      }
      this.eventSequence += 1;
      this.emit({
        eventId: this.eventSequence,
        timestamp: new Date().toISOString(),
        workspaceId: request.workspaceId as WorkspaceId,
        payload: {
          type: 'diskUsage.progress',
          scanId: request.scanId,
          root: result.root,
          unreadableEntries: 0,
          unreadable: [],
          scannedEntries: 1,
          isComplete: true,
        },
      });
      return undefined;
    });
  }

  cancelDiskUsage(_scanId: string, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelDiskUsage', signal, () => undefined);
  }

  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult> {
    return this.perform('discoverApplicationUninstallCandidates', signal, () => {
      const segments = request.location.uri.split('/');
      const rawName = segments[segments.length - 1] ?? '';
      const name = decodeURIComponent(rawName);
      if (!name.toLowerCase().endsWith('.app')) {
        throw new MockClientError(
          'notFound',
          `No mock application bundle at ${request.location.uri}`,
        );
      }
      const productName = name.slice(0, -'.app'.length);
      return {
        bundleIdentifier: `com.example.${productName.replace(/\s+/g, '')}`,
        productName,
        relatedFiles: [],
      };
    });
  }

  removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult> {
    // The mock world has no Dock to pin an icon to, so there is never anything to remove -
    // matching a real host's own normal "nothing was pinned" outcome, not an error.
    void request;
    return this.perform('removeApplicationDockIcon', signal, () => ({ removed: false }));
  }

  gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult> {
    // The mock fixtures have no notion of a git working tree, so every file simply has no
    // history to show - the same outcome a real backend reports for a non-git directory.
    void request;
    return this.perform('gitFileHistory', signal, () => ({ commits: [] }));
  }

  startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation> {
    return this.perform('startOperation', signal, () => {
      this.operationSequence += 1;
      const operation: Operation = {
        id: `mock-operation-${this.seed}-${this.operationSequence}`,
        kind: request.type,
        state: 'running',
        sources: request.sources.map((location) => ({ id: location.uri, location })),
        ...(request.destination === undefined ? {} : { destination: request.destination }),
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: request.conflictPolicy,
        createdAt: '2026-01-01T00:00:00.000Z',
        startedAt: '2026-01-01T00:00:00.000Z',
      };
      this.operations.set(operation.id, operation);
      return operation;
    });
  }

  listOperations(signal?: AbortSignal): Promise<Operation[]> {
    return this.perform('listOperations', signal, () =>
      [...this.operations.values()].map((operation) => structuredClone(operation)),
    );
  }

  cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelOperation', signal, () => {
      const operation = this.requireOperation(operationId);
      this.operations.set(operationId, { ...operation, state: 'cancelled' });
    });
  }

  undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation> {
    return this.perform('undoOperation', signal, () => {
      const original = this.requireOperation(operationId);
      if (original.undo?.available !== true) {
        throw new Error(original.undo?.reason ?? 'This operation cannot be undone.');
      }
      this.operationSequence += 1;
      const undo: Operation = {
        id: `mock-operation-${this.seed}-${this.operationSequence}`,
        kind: 'undo',
        state: 'running',
        sources: original.sources,
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: 'ask',
        createdAt: '2026-01-01T00:00:00.000Z',
        startedAt: '2026-01-01T00:00:00.000Z',
        undo: { available: false, reason: 'Undo operations cannot themselves be undone.' },
        undoOf: operationId,
      };
      this.operations.set(operationId, {
        ...original,
        undo: {
          available: false,
          reason: 'Undo is already in progress for this operation.',
          operationId: undo.id,
        },
      });
      this.operations.set(undo.id, undo);
      return undo;
    });
  }

  pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.perform('pauseOperation', signal, () => {
      const operation = this.requireOperation(operationId);
      this.operations.set(operationId, { ...operation, state: 'paused' });
    });
  }

  resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.perform('resumeOperation', signal, () => {
      const operation = this.requireOperation(operationId);
      this.operations.set(operationId, { ...operation, state: 'running' });
    });
  }

  resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('resolveConflict', signal, () => {
      const operation = this.requireOperation(request.operationId);
      this.operations.set(request.operationId, {
        ...operation,
        ...(request.resolution === 'cancelOperation'
          ? { state: 'cancelled' as const }
          : request.resolution === 'confirm'
            ? { state: 'running' as const }
            : { conflictPolicy: request.resolution, state: 'running' as const }),
      });
    });
  }

  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]> {
    return this.perform('listActions', signal, () => structuredClone(actions));
  }

  invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult> {
    return this.perform('invokeAction', signal, () => {
      if (!actions.some((action) => action.id === request.actionId)) {
        throw new MockClientError('actionNotFound', `No mock action with id ${request.actionId}`);
      }
      return { actionId: request.actionId, invoked: true };
    });
  }

  listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]> {
    return this.perform('listPlugins', signal, () => structuredClone(this.pluginState));
  }

  setPluginEnabled(pluginId: PluginId, enabled: boolean, signal?: AbortSignal): Promise<void> {
    return this.perform('setPluginEnabled', signal, () => {
      if (!this.pluginState.some((plugin) => plugin.id === pluginId)) {
        throw new MockClientError('pluginNotFound', `No mock plugin with id ${pluginId}`);
      }
      this.pluginState = this.pluginState.map((plugin) =>
        plugin.id === pluginId ? { ...plugin, enabled } : plugin,
      );
    });
  }

  getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]> {
    return this.perform('getPluginLogs', signal, () => {
      if (!this.pluginState.some((plugin) => plugin.id === pluginId)) {
        throw new MockClientError('pluginNotFound', `No mock plugin with id ${pluginId}`);
      }
      return [];
    });
  }

  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string> {
    return this.perform('getPluginIconThemeAsset', signal, () => {
      const plugin = this.pluginState.find((candidate) => candidate.id === pluginId);
      const isDeclared = Object.values(plugin?.iconTheme?.iconDefinitions ?? {}).some(
        (definition) => definition.iconPath === assetPath,
      );
      if (!isDeclared) {
        throw new MockClientError(
          'pluginNotFound',
          `No icon theme asset ${assetPath} for plugin ${pluginId}`,
        );
      }
      return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>';
    });
  }

  startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult> {
    return this.perform('startSearch', signal, () => {
      this.searchSequence += 1;
      const searchId = `mock-search-${this.seed}-${this.searchSequence}`;
      const location: Location = { providerId: 'local', uri: `search://local/${searchId}` };
      const roots = request.structuredQuery?.scope.locations ?? request.roots;
      const filenameQuery = request.structuredQuery?.name?.pattern ?? request.query;
      const contentQuery = request.structuredQuery?.content?.query ?? request.contentQuery;
      const entries = roots.flatMap((root) =>
        collectMatches(
          root.uri,
          filenameQuery,
          contentQuery,
          request.structuredQuery?.scope.showHidden ?? request.showHidden ?? true,
          (uri) => this.fileContentFor(uri),
          request.structuredQuery,
        ),
      );
      this.searches.set(searchId, { cancelled: false, entries });
      // Deferred with a macrotask (rather than a microtask) so it always runs
      // after this method's own promise has resolved and the caller has
      // recorded `searchId`, avoiding a race against the resultsBatch handler
      // matching events by searchId.
      setTimeout(() => {
        if (this.searches.get(searchId)?.cancelled ?? true) return;
        this.eventSequence += 1;
        this.emit({
          eventId: this.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'search.resultsBatch',
            searchId,
            entries,
            isComplete: true,
            warningsCount: 0,
            executionMode: 'liveRecursive',
          },
        });
      }, 0);
      const unsupported = [
        ...(request.structuredQuery?.gitStatuses.length ? (['gitStatus'] as const) : []),
        ...(request.structuredQuery?.tags.length ? (['tags'] as const) : []),
        ...(Object.keys(request.structuredQuery?.metadata ?? {}).length
          ? (['metadata'] as const)
          : []),
      ];
      return {
        searchId,
        location,
        limitations:
          unsupported.length === 0
            ? []
            : [{ providerId: 'local', unevaluatedPredicates: unsupported }],
        executionMode: 'liveRecursive',
      };
    });
  }

  cancelSearch(searchId: string, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelSearch', signal, () => {
      const search = this.searches.get(searchId);
      if (search === undefined) {
        throw new MockClientError('searchNotFound', `No mock search with id ${searchId}`);
      }
      search.cancelled = true;
    });
  }

  startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult> {
    return this.perform('startComparison', signal, () => {
      this.comparisonSequence += 1;
      const comparisonId = `mock-comparison-${this.seed}-${this.comparisonSequence}`;
      const entries = buildMockComparisonEntries(
        request.left.uri,
        request.right.uri,
        request.criteria,
        request.showHidden ?? false,
      );
      this.comparisons.set(comparisonId, {
        cancelled: false,
        entries,
        left: request.left,
        right: request.right,
        criteria: request.criteria,
      });
      // Deferred with a macrotask so it always runs after this method's own
      // promise resolves and the caller has recorded `comparisonId`,
      // mirroring `startSearch`'s race-avoidance for its results-batch event.
      setTimeout(() => {
        if (this.comparisons.get(comparisonId)?.cancelled ?? true) return;
        this.eventSequence += 1;
        this.emit({
          eventId: this.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'comparison.resultsBatch',
            comparisonId,
            entries,
            isComplete: true,
            warningsCount: 0,
          },
        });
      }, 0);
      return { comparisonId };
    });
  }

  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage> {
    return this.perform('getComparison', signal, () => {
      const comparison = this.requireComparison(comparisonId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      const filtered =
        (options?.differencesOnly ?? false)
          ? comparison.entries.filter((entry) => entry.status !== 'identical')
          : comparison.entries;
      return {
        comparisonId,
        left: comparison.left,
        right: comparison.right,
        criteria: comparison.criteria,
        offset,
        limit,
        total: filtered.length,
        entries: filtered.slice(offset, offset + limit),
        isComplete: true,
        warningsCount: 0,
      };
    });
  }

  cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelComparison', signal, () => {
      this.requireComparison(comparisonId).cancelled = true;
    });
  }

  startChecksums(
    request: StartChecksumRequest,
    signal?: AbortSignal,
  ): Promise<StartChecksumResult> {
    return this.perform('startChecksums', signal, () => {
      this.checksumSequence += 1;
      const jobId = `mock-checksum-${this.seed}-${this.checksumSequence}`;
      const entries: ChecksumEntry[] = request.entries.map((location) => {
        const content = this.fileContentFor(location.uri);
        const checksums: Record<string, string> = {};
        for (const algorithm of request.algorithms) {
          checksums[algorithm] = mockDigest(location.uri, algorithm);
        }
        return {
          location,
          relativePath: lastSegment(location.uri),
          size: content.byteLength,
          checksums,
        };
      });
      this.checksumJobs.set(jobId, {
        cancelled: false,
        entries,
        algorithms: request.algorithms,
      });
      // Deferred with a macrotask for the same reason as `startComparison`:
      // the caller must have recorded `jobId` before the batch arrives.
      setTimeout(() => {
        if (this.checksumJobs.get(jobId)?.cancelled ?? true) return;
        this.eventSequence += 1;
        this.emit({
          eventId: this.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'checksum.resultsBatch',
            jobId,
            entries,
            isComplete: true,
            isCancelled: false,
          },
        });
      }, 0);
      return { jobId };
    });
  }

  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage> {
    return this.perform('getChecksums', signal, () => {
      const job = this.requireChecksumJob(jobId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      return {
        jobId,
        algorithms: job.algorithms,
        offset,
        limit,
        total: job.entries.length,
        totalEntries: job.entries.length,
        entries: job.entries.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: job.cancelled,
        hasMore: offset + limit < job.entries.length,
      };
    });
  }

  cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelChecksums', signal, () => {
      this.requireChecksumJob(jobId).cancelled = true;
    });
  }

  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile> {
    return this.perform('renderChecksumFile', signal, () => {
      const job = this.requireChecksumJob(jobId);
      const lines = job.entries
        .filter((entry) => entry.checksums[algorithm] !== undefined)
        .map((entry) => `${entry.checksums[algorithm]}  ${entry.relativePath}`);
      return {
        suggestedName: `checksums.${algorithm}`,
        content: `# ${algorithm}\n${lines.join('\n')}\n`,
      };
    });
  }

  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile> {
    return this.perform('saveChecksumFile', signal, () => {
      const job = this.requireChecksumJob(jobId);
      const lines = job.entries
        .filter((entry) => entry.checksums[request.algorithm] !== undefined)
        .map((entry) => `${entry.checksums[request.algorithm]}  ${entry.relativePath}`);
      const content = `# ${request.algorithm}\n${lines.join('\n')}\n`;
      // The mock has no real filesystem; recording the bytes it would have
      // written is enough to exercise the UI's save flow.
      this.fileContents.set(request.destination.uri, new TextEncoder().encode(content));
      return { location: request.destination, bytesWritten: content.length };
    });
  }

  verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport> {
    return this.perform('verifyChecksumFile', signal, () => {
      const job = this.requireChecksumJob(jobId);
      const results: VerificationResult[] = [];
      let matched = 0;
      let mismatched = 0;
      let missing = 0;
      for (const line of content.split('\n')) {
        const trimmed = line.trim();
        if (trimmed === '' || trimmed.startsWith('#')) continue;
        const [digest, ...rest] = trimmed.split(/ {2}| \*/);
        const path = rest.join('  ');
        if (digest === undefined || path === '') continue;
        const entry = job.entries.find((candidate) => candidate.relativePath === path);
        const actual = entry === undefined ? undefined : Object.values(entry.checksums)[0];
        if (actual === undefined) {
          missing += 1;
          results.push({ path, status: 'missing' });
        } else if (actual.toLowerCase() === digest.toLowerCase()) {
          matched += 1;
          results.push({ path, status: 'match' });
        } else {
          mismatched += 1;
          results.push({ path, status: 'mismatch', expected: digest, actual });
        }
      }
      return { jobId, results, matched, mismatched, missing };
    });
  }

  startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult> {
    return this.perform('startDuplicateScan', signal, () => {
      this.duplicateScanSequence += 1;
      const scanId = `mock-duplicate-scan-${this.seed}-${this.duplicateScanSequence}`;
      const groups = buildMockDuplicateGroups(request.roots);
      this.duplicateScans.set(scanId, { cancelled: false, groups, roots: request.roots });
      setTimeout(() => {
        if (this.duplicateScans.get(scanId)?.cancelled ?? true) return;
        this.eventSequence += 1;
        this.emit({
          eventId: this.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'duplicates.resultsReady',
            scanId,
            groups,
            isCancelled: false,
            warningsCount: 0,
          },
        });
      }, 0);
      return { scanId };
    });
  }

  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage> {
    return this.perform('getDuplicateScan', signal, () => {
      const scan = this.requireDuplicateScan(scanId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      const fullyHashed = scan.groups.reduce(
        (total, group) => total + group.distinctLocations.length + group.hardlinkClusters.length,
        0,
      );
      return {
        scanId,
        roots: scan.roots,
        offset,
        limit,
        total: scan.groups.length,
        groups: scan.groups.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: scan.cancelled,
        hasMore: offset + limit < scan.groups.length,
        stats: {
          candidates: fullyHashed + 2,
          sizeSurvivors: fullyHashed,
          partiallyHashed: fullyHashed,
          fullyHashed,
          bytesHashed: scan.groups.reduce(
            (total, group) => total + group.size * group.distinctLocations.length,
            0,
          ),
          failed: 0,
        },
        warningsCount: 0,
      };
    });
  }

  cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.perform('cancelDuplicateScan', signal, () => {
      this.requireDuplicateScan(scanId).cancelled = true;
    });
  }

  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan> {
    return this.perform('generateSyncPlan', signal, () => {
      const comparison = this.requireComparison(comparisonId);
      const items = comparison.entries
        .filter((entry) => entry.status !== 'identical')
        .map((entry) => ({
          relativePath: entry.relativePath,
          status: entry.status,
          action: defaultSyncAction(entry.status, request.mode),
          ...(entry.left === undefined ? {} : { left: entry.left }),
          ...(entry.right === undefined ? {} : { right: entry.right }),
        }));
      return { comparisonId, items };
    });
  }

  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult> {
    return this.perform('applySyncPlan', signal, () => {
      const comparison = this.requireComparison(comparisonId);
      const operationIds: OperationId[] = [];
      for (const item of request.items) {
        if (item.action === 'skip') continue;
        this.operationSequence += 1;
        const operationId = `mock-operation-${this.seed}-${this.operationSequence}`;
        const isDelete = item.action === 'deleteLeft' || item.action === 'deleteRight';
        const sourceRoot =
          item.action === 'copyRightToLeft' || item.action === 'deleteRight'
            ? comparison.right
            : comparison.left;
        const source: Location = {
          providerId: sourceRoot.providerId,
          uri: `${sourceRoot.uri}/${item.relativePath}`,
        };
        this.operations.set(operationId, {
          id: operationId,
          kind: isDelete ? 'delete' : 'copy',
          state: 'completed',
          sources: [{ id: source.uri, location: source }],
          progress: { completedItems: 1, completedBytes: 0 },
          conflictPolicy: 'overwrite',
          createdAt: '2026-01-01T00:00:00.000Z',
          startedAt: '2026-01-01T00:00:00.000Z',
        });
        operationIds.push(operationId);
      }
      return { operationIds };
    });
  }

  subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    this.connection.set('open');
    this.listeners.add(listener);
    return Promise.resolve(() => {
      this.listeners.delete(listener);
    });
  }

  disconnect(): void {
    this.connection.set('closed');
  }

  onResynchronise(listener: () => void): Unsubscribe {
    return this.resynchronise.subscribe(listener);
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

  listConnections(signal?: AbortSignal): Promise<Connection[]> {
    return this.perform('listConnections', signal, () =>
      [...this.connections.values()].map((connection) => structuredClone(connection)),
    );
  }

  createConnection(request: CreateConnectionRequest, signal?: AbortSignal): Promise<Connection> {
    return this.perform('createConnection', signal, () => {
      this.connectionSequence += 1;
      const now = '2026-01-01T00:00:00.000Z';
      const connection: Connection = {
        id: `mock-connection-${this.connectionSequence}`,
        name: request.name,
        kind: request.kind,
        configuration: request.configuration,
        hasCredential: request.secret != null,
        status: 'disconnected',
        rootLocation:
          request.kind === 'oneDrive'
            ? `onedrive://mock-connection-${this.connectionSequence}/`
            : null,
        createdAt: now,
        updatedAt: now,
      };
      this.connections.set(connection.id, connection);
      return structuredClone(connection);
    });
  }

  getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.perform('getConnection', signal, () =>
      structuredClone(this.requireConnection(connectionId)),
    );
  }

  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection> {
    return this.perform('updateConnection', signal, () => {
      const existing = this.requireConnection(connectionId);
      const updated: Connection = {
        ...existing,
        name: request.name,
        kind: request.kind,
        configuration: request.configuration,
        hasCredential: request.secret != null ? true : existing.hasCredential,
        updatedAt: '2026-01-01T00:00:00.000Z',
      };
      this.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void> {
    return this.perform('deleteConnection', signal, () => {
      this.requireConnection(connectionId);
      this.connections.delete(connectionId);
    });
  }

  connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.perform('connectConnection', signal, () => {
      const connection = this.requireConnection(connectionId);
      const updated: Connection = {
        ...connection,
        status: evaluateMockConnectionStatus(connection),
      };
      this.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  disconnectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.perform('disconnectConnection', signal, () => {
      const connection = this.requireConnection(connectionId);
      const updated: Connection = { ...connection, status: 'disconnected' };
      this.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  /** Evaluates status without persisting it, mirroring the backend's `test` semantics. */
  testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.perform('testConnection', signal, () => {
      const connection = this.requireConnection(connectionId);
      return structuredClone({ ...connection, status: evaluateMockConnectionStatus(connection) });
    });
  }

  beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse> {
    return this.perform('beginOneDriveAuthorization', signal, () => {
      const connection = this.requireConnection(connectionId);
      if (connection.configuration.kind !== 'oneDrive') {
        throw new MockClientError('invalidRequest', 'Only OneDrive connections can be authorized');
      }
      this.oneDriveAuthorizationSequence += 1;
      const attemptId = `mock-onedrive-authorization-${this.oneDriveAuthorizationSequence}`;
      this.oneDriveAuthorizations.set(attemptId, {
        connectionId,
        attempt: { id: attemptId, status: { state: 'pending' } },
      });
      return {
        attemptId,
        authorizationUrl: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize?state=${attemptId}`,
      };
    });
  }

  getOneDriveAuthorizationAttempt(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return this.perform('getOneDriveAuthorizationAttempt', signal, () => {
      const authorization = this.oneDriveAuthorizations.get(attemptId);
      if (authorization === undefined) {
        throw new MockClientError('notFound', `No mock OneDrive authorization ${attemptId}`);
      }
      if (authorization.attempt.status.state === 'pending') {
        const connection = this.requireConnection(authorization.connectionId);
        if (connection.configuration.kind !== 'oneDrive') {
          throw new MockClientError(
            'invalidRequest',
            'Connection kind changed during authorization',
          );
        }
        const email = connection.configuration.accountHint ?? 'mock.user@example.test';
        const authorized: Connection = {
          ...connection,
          configuration: {
            ...connection.configuration,
            displayName: 'Mock Microsoft User',
            email,
            driveType: connection.configuration.accountHint === null ? 'personal' : 'business',
          },
          hasCredential: true,
          status: 'connected',
          rootLocation: `onedrive://${connection.id}/`,
          updatedAt: '2026-01-01T00:00:00.000Z',
        };
        this.connections.set(connection.id, authorized);
        authorization.attempt = {
          id: attemptId,
          status: { state: 'succeeded', connection: authorized },
        };
      }
      return structuredClone(authorization.attempt);
    });
  }

  cancelOneDriveAuthorization(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return this.perform('cancelOneDriveAuthorization', signal, () => {
      const authorization = this.oneDriveAuthorizations.get(attemptId);
      if (authorization === undefined) {
        throw new MockClientError('notFound', `No mock OneDrive authorization ${attemptId}`);
      }
      if (authorization.attempt.status.state === 'pending') {
        authorization.attempt = { id: attemptId, status: { state: 'cancelled' } };
      }
      return structuredClone(authorization.attempt);
    });
  }

  /**
   * Mock mode never performs a real network dial, so there is no host key to
   * present - every connection reports as already trusted, matching
   * `evaluateMockConnectionStatus` never producing `hostKeyUnverified`/
   * `hostKeyMismatch`.
   */
  probeSshHostKey(connectionId: ConnectionId, signal?: AbortSignal): Promise<HostKeyProbe> {
    return this.perform('probeSshHostKey', signal, () => {
      this.requireConnection(connectionId);
      return { status: 'trusted', fingerprint: 'SHA256:mock-fingerprint' };
    });
  }

  acceptSshHostKey(
    connectionId: ConnectionId,
    _fingerprint: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.perform('acceptSshHostKey', signal, () => {
      this.requireConnection(connectionId);
    });
  }

  private requireConnection(connectionId: ConnectionId): Connection {
    const connection = this.connections.get(connectionId);
    if (connection === undefined) {
      throw new MockClientError('notFound', `No mock connection with id ${connectionId}`);
    }
    return connection;
  }

  /** Returns the current in-memory state for a mock operation. */
  getOperation(operationId: OperationId): Operation | undefined {
    const operation = this.operations.get(operationId);
    return operation === undefined ? undefined : structuredClone(operation);
  }

  private structuredSession(sessionId: string) {
    const session = this.structuredSessions.get(sessionId);
    if (session === undefined) {
      throw new MockClientError('notFound', 'structured viewer session not found');
    }
    return session;
  }

  private mockStructuredView(sessionId: string): StructuredView {
    const session = this.structuredSession(sessionId);
    const bytes = this.fileContentFor(session.uri);
    const externalFallback = session.format === 'excel';
    const jsonText = session.format === 'json';
    const records =
      externalFallback || jsonText
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
    const useHeader = session.headerMode === 'firstRow' || session.headerMode === 'auto';
    const headers = useHeader
      ? (records[0] ?? [])
      : (records[0]?.map((_, index) => `Column ${index + 1}`) ?? []);
    const dataRecords = useHeader ? records.slice(1) : records;
    const rows = dataRecords.slice(0, 500).map((cells, index) => ({ index, cells }));
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
      indexedBytes: bytes.length,
      indexedRows: dataRecords.length,
      totalRows: dataRecords.length,
      indexingComplete: true,
      warning: externalFallback
        ? "This workbook cannot be opened within the viewer's bounded-memory budget. Open it in an external spreadsheet application."
        : null,
    };
  }

  private directorySnapshot(
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
        new MockClientError('directoryNotFound', `No mock directory at ${request.location.uri}`),
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
      ? ({ type: 'error', message: 'Directory is not readable' } as const)
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

  private generatedDirectoryTotals(size: GeneratedDirectorySize): {
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

  private generatedSize(uri: string): GeneratedDirectorySize | undefined {
    const match = /^mock:\/\/\/large\/(\d+)$/.exec(uri);
    if (match?.[1] === undefined) {
      return undefined;
    }
    const size = Number(match[1]);
    return GENERATED_DIRECTORY_SIZES.find((candidate) => candidate === size);
  }

  private parseContinuationToken(token: string | undefined): number {
    if (token === undefined) {
      return 0;
    }
    const offset = Number(token);
    if (!Number.isSafeInteger(offset) || offset < 0) {
      throw new MockClientError('invalidContinuationToken', `Invalid continuation token: ${token}`);
    }
    return offset;
  }

  private requireOperation(operationId: OperationId): Operation {
    const operation = this.operations.get(operationId);
    if (operation === undefined) {
      throw new MockClientError('operationNotFound', `No mock operation with id ${operationId}`);
    }
    return operation;
  }

  private requireChecksumJob(jobId: string): {
    cancelled: boolean;
    entries: readonly ChecksumEntry[];
    algorithms: readonly ChecksumAlgorithm[];
  } {
    const job = this.checksumJobs.get(jobId);
    if (job === undefined) {
      throw new MockClientError('checksumJobNotFound', `No mock checksum job with id ${jobId}`);
    }
    return job;
  }

  private requireDuplicateScan(scanId: string): {
    cancelled: boolean;
    groups: readonly DuplicateGroup[];
    roots: readonly Location[];
  } {
    const scan = this.duplicateScans.get(scanId);
    if (scan === undefined) {
      throw new MockClientError(
        'duplicateScanNotFound',
        `No mock duplicate scan with id ${scanId}`,
      );
    }
    return scan;
  }

  private requireComparison(comparisonId: string): {
    cancelled: boolean;
    entries: readonly ComparisonEntry[];
    left: Location;
    right: Location;
    criteria: ComparisonCriteria;
  } {
    const comparison = this.comparisons.get(comparisonId);
    if (comparison === undefined) {
      throw new MockClientError('comparisonNotFound', `No mock comparison with id ${comparisonId}`);
    }
    return comparison;
  }

  private fileContentFor(uri: string): Uint8Array {
    let content = this.fileContents.get(uri);
    if (content === undefined) {
      content = syntheticFileContent(uri);
      this.fileContents.set(uri, content);
    }
    return content;
  }

  /** Builds a per-line match finder for a search request, mirroring the backend's
   * substring/regex, case-(in)sensitive `ContentQuery` semantics closely enough for mock/dev use. */
  private buildLineMatcher(request: SearchInFileRequest): (line: string) => [number, number][] {
    if (request.regex) {
      const source = request.wholeWord ? `\\b(?:${request.query})\\b` : request.query;
      let pattern: RegExp;
      try {
        pattern = new RegExp(source, request.caseSensitive ? 'gu' : 'giu');
      } catch (error) {
        throw new MockClientError(
          'invalidRequest',
          `invalid regular expression: ${(error as Error).message}`,
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

  private async perform<T>(
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

  private delay(signal: AbortSignal | undefined): Promise<void> {
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

function parseMockDelimited(text: string, delimiter: string): string[][] {
  const records: string[][] = [];
  let record: string[] = [];
  let field = '';
  let quoted = false;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character === '"') {
      if (quoted && text[index + 1] === '"') {
        field += '"';
        index += 1;
      } else {
        quoted = !quoted;
      }
    } else if (character === delimiter && !quoted) {
      record.push(field);
      field = '';
    } else if ((character === '\n' || character === '\r') && !quoted) {
      if (character === '\r' && text[index + 1] === '\n') index += 1;
      record.push(field);
      records.push(record);
      record = [];
      field = '';
    } else {
      field += character;
    }
  }
  if (field.length > 0 || record.length > 0) {
    record.push(field);
    records.push(record);
  }
  return records;
}

/** A deterministic error raised by an injected or fixture-backed mock failure. */
export class MockClientError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'MockClientError';
  }
}
