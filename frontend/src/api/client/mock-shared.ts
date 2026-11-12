import actionFixtures from '../../../../fixtures/mock-responses/actions.json';
import directoryFixtures from '../../../../fixtures/mock-responses/directories.json';
import pluginFixtures from '../../../../fixtures/mock-responses/plugins.json';
import type {
  ActionDescriptor,
  ChecksumAlgorithm,
  ComparisonCriteria,
  ComparisonEntry,
  ComparisonEntrySide,
  ComparisonStatus,
  Connection,
  DuplicateGroup,
  EntrySummary,
  GenerateSyncPlanRequest,
  Location,
  PluginDescriptor,
  SearchQuery,
  SemanticComponentLifecycle,
  SemanticComponentOperation,
  SemanticComponentStatus,
  SemanticDeletionCategoryStatus,
  SemanticDiskUse,
  SemanticIndexRecordCounts,
  SemanticInstallationOffer,
  SemanticLibraryStatus,
  SemanticModelIdentity,
  SemanticModelMigrationProgress,
  SemanticModelProfile,
  SemanticModelSelection,
  SemanticOcrStatus,
  SemanticOcrTarget,
  SemanticProfile,
  SyncPlan,
  WorkspaceId,
  WorkspaceProjection,
} from '../../models';
import { escapeRegExp } from '../../utilities/regex';
import { bindLocalisedMessage } from '../error-localiser';

export interface FixtureEntry {
  name: string;
  kind: 'file' | 'directory' | 'symlink';
  size?: number;
  hidden?: boolean;
  readable?: boolean;
}

export const directories = directoryFixtures as Record<string, FixtureEntry[]>;
export const actions = actionFixtures as ActionDescriptor[];
export const plugins = pluginFixtures as PluginDescriptor[];

/** Extensions {@link MockFileManagerClient.getThumbnail} fakes a preview for (task 0134). */
export const THUMBNAILABLE_MOCK_EXTENSIONS = new Set([
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
  | 'getSemanticComponentCapabilities'
  | 'getSemanticComponentStatus'
  | 'listSemanticComponentProfiles'
  | 'initializeGemmaLibrary'
  | 'getGemmaLibrarySetup'
  | 'createSemanticComponentInstallationOffer'
  | 'acceptSemanticComponentInstallationOffer'
  | 'pauseSemanticComponentIndexing'
  | 'resumeSemanticComponentIndexing'
  | 'createSemanticComponentIndexRemovalPlan'
  | 'confirmSemanticComponentIndexRemoval'
  | 'moveSemanticComponentData'
  | 'uninstallSemanticComponents'
  | 'installSemanticComponentWorkerPatch'
  | 'importSemanticComponentLocalModel'
  | 'planSemanticComponentModelMigration'
  | 'confirmSemanticComponentModelMigration'
  | 'checkpointSemanticComponentModelMigration'
  | 'completeSemanticComponentModelMigration'
  | 'getSemanticOcrStatus'
  | 'setSemanticOcrConsent'
  | 'setSemanticOcrReplaceOriginal'
  | 'startSemanticOcrRemediation'
  | 'cancelSemanticOcrRemediation'
  | 'getSemanticLibraryCapabilities'
  | 'getSemanticLibraryStatus'
  | 'updateGemmaMedia'
  | 'listSemanticVocabularies'
  | 'importSemanticVocabulary'
  | 'exportSemanticVocabulary'
  | 'attachSemanticVocabulary'
  | 'reviewSemanticConceptCandidate'
  | 'deleteSemanticVocabulary'
  | 'getSemanticFolderStatus'
  | 'previewSemanticEnrolment'
  | 'confirmSemanticEnrolment'
  | 'planSemanticExclusion'
  | 'confirmSemanticExclusion'
  | 'resumeSemanticCleanup'
  | 'pauseSemanticLibrary'
  | 'resumeSemanticLibrary'
  | 'updateSemanticEligibilityOverrides'
  | 'updateSemanticRootWorkspaces'
  | 'getDiagnostics'
  | 'recordFrontendDiagnostic'
  | 'getSystemLocations'
  | 'getVolumes'
  | 'getHomeDirectory'
  | 'startNativeDrag'
  | 'writeOsClipboardFiles'
  | 'readOsClipboardFiles'
  | 'showPlatformContextMenu'
  | 'getSettings'
  | 'listFileTemplates'
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
  | 'listArchiveEdits'
  | 'resolveArchiveEdit'
  | 'readFileRange'
  | 'previewGeoConversion'
  | 'previewGeoMinify'
  | 'convertGeoText'
  | 'parseGeoText'
  | 'readGeoFile'
  | 'readImageGps'
  | 'readImageMetadata'
  | 'inspectAttributes'
  | 'inspectSplitSet'
  | 'diffFiles'
  | 'previewAttributeChange'
  | 'resolveMetadataTokens'
  | 'openDocxPreview'
  | 'readDocxPreviewResource'
  | 'closeDocxPreview'
  | 'openPptxPreview'
  | 'readPptxPreviewPdf'
  | 'closePptxPreview'
  | 'searchInFile'
  | 'calculateFolderSize'
  | 'archiveSummary'
  | 'scanDiskUsage'
  | 'cancelDiskUsage'
  | 'discoverApplicationUninstallCandidates'
  | 'removeApplicationDockIcon'
  | 'gitFileHistory'
  | 'startOperation'
  | 'getLinkOptions'
  | 'findTransferConflicts'
  | 'resolveLinkTarget'
  | 'listOperations'
  | 'listInterruptedTransfers'
  | 'restartInterruptedTransfer'
  | 'discardInterruptedTransfer'
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
  | 'renderPluginPreview'
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
  | 'startMusicDuplicateScan'
  | 'getMusicDuplicateScan'
  | 'cancelMusicDuplicateScan'
  | 'forgetFolderView'
  | 'startSnapshotJob'
  | 'getSnapshotJob'
  | 'cancelSnapshotJob'
  | 'startLinkDoctorScan'
  | 'getLinkDoctorScan'
  | 'cancelLinkDoctorScan'
  | 'findLinkReplacements'
  | 'listLlmProfilePresets'
  | 'listLlmProfiles'
  | 'streamFileAssistantReply'
  | 'createLlmProfile'
  | 'updateLlmProfile'
  | 'deleteLlmProfile'
  | 'cloneLlmProfile'
  | 'exportLlmProfile'
  | 'activateLlmProfile'
  | 'testLlmProfile'
  | 'discoverLlmProfileModels'
  | 'discoverLlmProfileDraftModels'
  | 'discoverLlmProfileDraftModelLimits'
  | 'previewDocumentSummary'
  | 'generateDocumentSummary'
  | 'getDocumentSummary'
  | 'getCorpusThemeStatus'
  | 'analyzeCorpusThemes'
  | 'exportCorpusThemes'
  | 'getCorpusThemeSources'
  | 'previewRag'
  | 'generateRagAnswer'
  | 'saveRagConversation'
  | 'listSavedRagConversations'
  | 'deleteRagConversation'
  | 'resolveRagCitation'
  | 'getKnowledgeCapabilities'
  | 'listKnowledgeRoots'
  | 'parseKnowledgeQuery'
  | 'planKnowledgeSearch'
  | 'executeKnowledgeSearch'
  | 'cancelKnowledgeSearch'
  | 'resolveKnowledgeSource'
  | 'generateKnowledgeAnswer'
  | 'cancelKnowledgeAnswer'
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

export type MockSemanticLifecycle =
  | 'unavailable'
  | 'absent'
  | 'offered'
  | 'downloading'
  | 'downloadingResumable'
  | 'installedEnabled'
  | 'paused'
  | 'migrating'
  | 'updateFailedRolledBack'
  | 'lowDisk'
  | 'uninstalledRetain'
  | 'uninstalledDelete';

export interface MockFileManagerClientOptions {
  pageSize?: number;
  seed?: number;
  loadingLocations?: readonly string[];
  latencyMs?: number;
  failures?: Partial<Record<MockClientMethod, Error>>;
  nativeIconExtensions?: readonly string[];
  semanticLifecycle?: MockSemanticLifecycle;
  semanticOcrStatus?: SemanticOcrStatus;
}

export function fixtureEntry(
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
export function aggregateTotals(entries: Iterable<import('../../models').EntrySummary>): {
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
export function matchesQuery(
  name: string,
  query: string,
  predicate?: SearchQuery['name'],
): boolean {
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

export function firstContentMatch(
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
    const escaped = escapeRegExp(query);
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
export function syntheticFileContent(uri: string): Uint8Array {
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
export function collectMatches(
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
export function walkFixtureTree(rootUri: string, showHidden: boolean): Map<string, EntrySummary> {
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

export function comparisonEntrySideFor(entry: EntrySummary): ComparisonEntrySide {
  return {
    kind: entry.kind,
    ...(entry.size === undefined ? {} : { size: entry.size }),
  };
}

/** The last `/`-separated segment of a URI, percent-decoded. */
export function lastSegment(uri: string): string {
  const trimmed = uri.endsWith('/') ? uri.slice(0, -1) : uri;
  const index = trimmed.lastIndexOf('/');
  return decodeURIComponent(index === -1 ? trimmed : trimmed.slice(index + 1));
}

export function mockSummaryVisualCount(uri: string): number {
  const name = lastSegment(uri.split(/[?#]/, 1)[0] ?? '').toLowerCase();
  return new Set([
    'report.docx',
    'illustrated.pdf',
    'presentation.pptx',
    'book.epub',
    'figures.xlsx',
  ]).has(name)
    ? 1
    : 0;
}

/**
 * A deterministic, plausible-looking digest for the mock runtime.
 *
 * Deliberately *not* a real hash: the mock never reads file bytes, and a
 * digest that merely looks right is enough to exercise the UI. It is stable
 * for a given (uri, algorithm) pair so repeated runs and the verify flow
 * agree with themselves.
 */
export function mockDigest(uri: string, algorithm: ChecksumAlgorithm): string {
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
export function buildMockDuplicateGroups(roots: readonly Location[]): DuplicateGroup[] {
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
export function buildMockComparisonEntries(
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
export function defaultSyncAction(
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

export function createMockWorkspace(id: WorkspaceId, name = 'Mock Workspace'): WorkspaceProjection {
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
export function evaluateMockConnectionStatus(connection: Connection): Connection['status'] {
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

export const SEMANTIC_OPERATIONS: readonly SemanticComponentOperation[] = [
  'viewStatus',
  'viewCatalog',
  'createInstallationOffer',
  'installOrEnable',
  'installWorkerPatch',
  'pauseIndexing',
  'resumeIndexing',
  'removeIndex',
  'moveData',
  'uninstallComponents',
  'importLocalModel',
  'planModelMigration',
  'confirmModelMigration',
  'checkpointModelMigration',
  'completeModelMigration',
];

export const MOCK_SEMANTIC_ENROLMENT_INVENTORY: Readonly<
  Record<string, SemanticIndexRecordCounts>
> = {
  'enrolment-1': {
    indexRecords: 3,
    extractedFiles: 2,
    zvecVectors: 3,
    cacheEntries: 1,
    conversationEvidence: 2,
  },
  'library-1': {
    indexRecords: 7,
    extractedFiles: 6,
    zvecVectors: 5,
    cacheEntries: 4,
    conversationEvidence: 3,
  },
};

export function sameSemanticIndexCounts(
  left: SemanticIndexRecordCounts,
  right: SemanticIndexRecordCounts,
): boolean {
  return (
    left.indexRecords === right.indexRecords &&
    left.extractedFiles === right.extractedFiles &&
    left.zvecVectors === right.zvecVectors &&
    left.cacheEntries === right.cacheEntries &&
    left.conversationEvidence === right.conversationEvidence
  );
}

export function mockSemanticIdentity(profile: SemanticProfile): SemanticModelIdentity {
  const suffix = {
    compactMultilingual: 'compact-multilingual',
    compactEnglish: 'compact-english',
    multilingualQuality: 'multilingual-quality',
    embeddingGemma2: 'embeddinggemma-2',
  }[profile];
  return {
    modelId: `mock-${suffix}`,
    revision: `mock-${suffix}-revision`,
  };
}

export function mockSemanticSelection(profile: SemanticProfile): SemanticModelSelection {
  return { profile, identity: mockSemanticIdentity(profile) };
}

export function mockSemanticProfiles(): SemanticModelProfile[] {
  const profiles: readonly SemanticProfile[] = [
    'compactMultilingual',
    'compactEnglish',
    'multilingualQuality',
  ];
  return profiles.map((profile) => {
    const identity = mockSemanticIdentity(profile);
    return {
      profile,
      recommended: profile === 'compactMultilingual',
      explanation: `Deterministic ${profile} mock profile.`,
      resolvedModel: identity,
      metadata: {
        identity,
        license: {
          spdx: 'MIT',
          notice: 'Deterministic mock model; no package is downloaded.',
        },
        tokenizer: `mock-${profile}-tokenizer`,
        dimensions: profile === 'multilingualQuality' ? 768 : 384,
        normalization: 'unitLength',
        runtimeComponentId: 'mock-runtime',
        runtimeVersionRequirement: '^1.0',
        languageCoverage: profile === 'compactEnglish' ? ['en'] : ['en', 'nl'],
        estimatedDiskBytes: profile === 'multilingualQuality' ? 600 : 300,
        estimatedRamBytes: profile === 'multilingualQuality' ? 800 : 400,
      },
    };
  });
}

export function mockSemanticInstalledComponents(): SemanticComponentStatus['components'] {
  return [
    {
      artifactId: 'mock-worker-artifact',
      componentId: 'mock-worker',
      kind: 'worker',
      version: '1.0.0',
      state: 'active',
      installedBytes: 60,
    },
    {
      artifactId: 'mock-runtime-artifact',
      componentId: 'mock-runtime',
      kind: 'runtime',
      version: '1.0.0',
      state: 'active',
      installedBytes: 70,
    },
    {
      artifactId: 'mock-model-artifact',
      componentId: 'mock-model',
      kind: 'model',
      version: '1.0.0',
      state: 'active',
      installedBytes: 300,
    },
  ];
}

export function emptySemanticDiskUse(): SemanticDiskUse {
  return {
    categories: [
      { category: 'catalog', bytes: 0 },
      { category: 'extracted', bytes: 0 },
      { category: 'zvec', bytes: 0 },
      { category: 'embeddingCache', bytes: 0 },
      { category: 'models', bytes: 0 },
      { category: 'workers', bytes: 0 },
    ],
    totalBytes: 0,
  };
}

export function installedSemanticDiskUse(): SemanticDiskUse {
  return {
    categories: [
      { category: 'catalog', bytes: 1 },
      { category: 'extracted', bytes: 0 },
      { category: 'zvec', bytes: 0 },
      { category: 'embeddingCache', bytes: 0 },
      { category: 'models', bytes: 300 },
      { category: 'workers', bytes: 130 },
    ],
    totalBytes: 431,
  };
}

export function retainedSemanticDiskUse(): SemanticDiskUse {
  return {
    categories: [
      { category: 'catalog', bytes: 1 },
      { category: 'extracted', bytes: 10 },
      { category: 'zvec', bytes: 30 },
      { category: 'embeddingCache', bytes: 10 },
      { category: 'models', bytes: 0 },
      { category: 'workers', bytes: 0 },
    ],
    totalBytes: 51,
  };
}

export function mockSemanticMigrationProgress(
  migrationId = 'mock-scenario-migration',
  completedDocuments = 4,
  estimate = { documents: 10, sourceBytes: 100 },
  target = mockSemanticSelection('multilingualQuality'),
  resumeCursor: string | null = 'mock-resume-cursor',
): SemanticModelMigrationProgress {
  return {
    migrationId,
    completedDocuments,
    estimate,
    target,
    reason: { reason: 'modelChanged' },
    resumeCursor,
  };
}

export function mockSemanticStatus(lifecycleName: MockSemanticLifecycle): SemanticComponentStatus {
  const installed =
    lifecycleName === 'installedEnabled' ||
    lifecycleName === 'paused' ||
    lifecycleName === 'migrating' ||
    lifecycleName === 'updateFailedRolledBack';
  const progress = lifecycleName === 'migrating' ? mockSemanticMigrationProgress() : undefined;
  const lifecycle: SemanticComponentLifecycle = (() => {
    switch (lifecycleName) {
      case 'unavailable':
        return { state: 'unavailable' };
      case 'absent':
        return { state: 'absent' };
      case 'offered':
        return { state: 'offered', offerId: 'mock-scenario-offer' };
      case 'downloading':
        return {
          state: 'downloading',
          downloadedBytes: 160,
          totalBytes: 400,
          resumable: false,
        };
      case 'downloadingResumable':
        return {
          state: 'downloading',
          downloadedBytes: 160,
          totalBytes: 400,
          resumable: true,
        };
      case 'installedEnabled':
        return { state: 'installedEnabled' };
      case 'paused':
        return { state: 'paused' };
      case 'migrating':
        return { state: 'migrating', progress: progress as SemanticModelMigrationProgress };
      case 'updateFailedRolledBack':
        return {
          state: 'updateFailedRolledBack',
          failedVersion: '1.0.1',
          activeVersion: '1.0.0',
        };
      case 'lowDisk':
        return { state: 'lowDisk', availableBytes: 512, requiredBytes: 1_024 };
      case 'uninstalledRetain':
        return { state: 'uninstalled', indexDecision: 'retain' };
      case 'uninstalledDelete':
        return { state: 'uninstalled', indexDecision: 'delete' };
    }
  })();
  const retainsIndex = lifecycleName === 'uninstalledRetain';
  return {
    lifecycle,
    dataRoot: lifecycleName === 'unavailable' ? null : 'mock/semantic',
    activeModel: installed || retainsIndex ? mockSemanticSelection('compactMultilingual') : null,
    migration: progress ?? null,
    components: installed ? mockSemanticInstalledComponents() : [],
    diskUse: installed
      ? installedSemanticDiskUse()
      : retainsIndex
        ? retainedSemanticDiskUse()
        : emptySemanticDiskUse(),
  };
}

export function mockSemanticOffer(
  offerId: string,
  profile: SemanticProfile,
): SemanticInstallationOffer {
  const resolvedModel = mockSemanticIdentity(profile);
  const license = {
    spdx: 'MIT',
    notice: 'Deterministic mock component; no package is downloaded.',
  };
  return {
    offerId,
    catalogRevision: 'mock-signed-catalog-revision',
    profile,
    resolvedModel,
    components: [
      {
        artifactId: 'mock-worker-artifact',
        componentId: 'mock-worker',
        kind: 'worker',
        model: null,
        version: '1.0.0',
        license,
        downloadBytes: 100,
        estimatedInstalledBytes: 200,
        estimatedRamBytes: 50,
      },
      {
        artifactId: 'mock-runtime-artifact',
        componentId: 'mock-runtime',
        kind: 'runtime',
        model: null,
        version: '1.0.0',
        license,
        downloadBytes: 100,
        estimatedInstalledBytes: 200,
        estimatedRamBytes: 100,
      },
      {
        artifactId: 'mock-model-artifact',
        componentId: 'mock-model',
        kind: 'model',
        model: resolvedModel,
        version: '1.0.0',
        license,
        downloadBytes: 200,
        estimatedInstalledBytes: 300,
        estimatedRamBytes: 400,
      },
    ],
    embeddingsStayLocal: true,
    localOnlyDisclosure: 'Embedding inference and semantic index data stay on this device.',
    dataRoot: 'mock/semantic',
    minimumFreeSpaceReserveBytes: 1_024,
  };
}

export const MOCK_SEMANTIC_DELETION_CATEGORIES = [
  'occurrences',
  'extractedContent',
  'summaries',
  'labels',
  'orphanVectors',
  'conversationEvidencePins',
] as const;

export function mockSemanticLibraryStatus(): SemanticLibraryStatus {
  return {
    available: true,
    revision: 1,
    paused: false,
    library: {
      libraryId: '00000000-0000-0000-0000-000000000179',
      model: {
        modelId: 'mock-semantic-model',
        revision: 'mock-revision-1',
        dimensions: 384,
        embeddingSpace: 'mock-embedding-space',
      },
    },
    resourceProfile: {
      kind: 'balanced',
      budgets: {
        maxDocuments: 1_000_000,
        maxSourceBytesPerDocument: 512 * 1_024 * 1_024,
        maxTotalSourceBytes: 4 * 1_024 * 1_024 * 1_024 * 1_024,
        maxTotalExtractedBytes: 1_024 * 1_024 * 1_024 * 1_024,
        maxTotalVectorBytes: 1_024 * 1_024 * 1_024 * 1_024,
      },
    },
    reconciliationIntervalSeconds: 1_800,
    roots: [],
    normalizedExcerptsRetainedLocally: true,
  };
}

export function mockSemanticOcrStatus(): SemanticOcrStatus {
  return {
    enabled: false,
    replaceOriginal: false,
    availability: {
      state: 'available',
      executable: '/usr/local/bin/ocrmypdf',
      version: '16.10.4',
    },
    reportedFiles: [
      {
        rootId: 'mock-ocr-root',
        location: { providerId: 'file', uri: 'file:///Documents/scanned-invoice.pdf' },
      },
      {
        rootId: 'mock-ocr-root',
        location: { providerId: 'file', uri: 'file:///Documents/scanned-notes.pdf' },
      },
    ],
    jobs: [],
  };
}

export function sameSemanticOcrTarget(left: SemanticOcrTarget, right: SemanticOcrTarget): boolean {
  return (
    left.rootId === right.rootId &&
    left.location.providerId === right.location.providerId &&
    left.location.uri === right.location.uri
  );
}

export function semanticLocationContains(root: Location, candidate: Location): boolean {
  if (root.providerId !== candidate.providerId) return false;
  if (root.uri === candidate.uri) return true;
  const prefix = root.uri.endsWith('/') ? root.uri : `${root.uri}/`;
  return candidate.uri.startsWith(prefix);
}

export function mockCleanupCategories(complete: boolean): SemanticDeletionCategoryStatus[] {
  return MOCK_SEMANTIC_DELETION_CATEGORIES.map((category, index) => ({
    category,
    totalItems: index + 1,
    completedItems: complete ? index + 1 : 0,
    complete,
    lastError: null,
  }));
}

/** Strictly typed controls for the deterministic in-memory frontend adapter. */

export function parseMockDelimited(text: string, delimiter: string): string[][] {
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
    readonly details?: Readonly<Record<string, unknown>>,
  ) {
    super(message);
    this.name = 'MockClientError';
    bindLocalisedMessage(this, code, details, message);
  }
}
