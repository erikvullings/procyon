import actionFixtures from '../../../../fixtures/mock-responses/actions.json';
import directoryFixtures from '../../../../fixtures/mock-responses/directories.json';
import pluginFixtures from '../../../../fixtures/mock-responses/plugins.json';
import type {
  AcceptSemanticInstallationOfferRequest,
  ActionDescriptor,
  ActionResult,
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ArchiveCredentialRequest,
  ArchiveSummaryRequest,
  ArchiveSummaryResult,
  AttachSemanticVocabularyRequest,
  BackendEvent,
  BeginOneDriveAuthorizationResponse,
  CalculateFolderSizeRequest,
  CalculateFolderSizeResult,
  CancelKnowledgeAnswerRequest,
  CancelKnowledgeSearchRequest,
  CheckpointSemanticModelMigrationRequest,
  ChecksumAlgorithm,
  ChecksumEntry,
  ChecksumFile,
  ChecksumPage,
  ComparisonCriteria,
  ComparisonEntry,
  ComparisonEntrySide,
  ComparisonPage,
  ComparisonStatus,
  CompleteSemanticModelMigrationRequest,
  ConfirmSemanticEnrolmentRequest,
  ConfirmSemanticExclusionRequest,
  ConfirmSemanticIndexRemovalRequest,
  ConfirmSemanticModelMigrationRequest,
  Connection,
  ConnectionId,
  CreateConnectionRequest,
  CreateSemanticIndexRemovalPlanRequest,
  CreateSemanticInstallationOfferRequest,
  CreateWorkspaceRequest,
  DeleteLlmProfileRequest,
  DeleteRagConversationRequest,
  DeleteSemanticVocabularyImpact,
  DiagnosticsResult,
  DirectorySnapshot,
  DiscoverApplicationUninstallCandidatesRequest,
  DiscoverApplicationUninstallCandidatesResult,
  DocumentSummary,
  DocumentSummaryPreview,
  DocxPreview,
  DocxPreviewResource,
  DocxPreviewSessionRequest,
  DuplicateGroup,
  DuplicatePage,
  EditableFile,
  EditableFileSave,
  EntryMetadata,
  EntryMetadataRequest,
  EntrySummary,
  ExecuteKnowledgeSearchRequest,
  FileRangeChunk,
  FinderTags,
  GenerateDocumentSummaryRequest,
  GenerateKnowledgeAnswerRequest,
  GenerateRagAnswerRequest,
  GenerateRagAnswerResponse,
  GenerateSyncPlanRequest,
  GetDocumentSummaryRequest,
  GetSemanticFolderStatusRequest,
  GitFileHistoryRequest,
  GitFileHistoryResult,
  HostKeyProbe,
  ImportSemanticLocalModelRequest,
  InstallSemanticWorkerPatchRequest,
  InvokeActionRequest,
  KnowledgeAnswer,
  KnowledgeCapabilities,
  KnowledgeQueryInterpretation,
  KnowledgeRoot,
  KnowledgeScope,
  KnowledgeSearchPlan,
  KnowledgeSearchResult,
  KnowledgeSourceLocation,
  ListDirectoryRequest,
  ListKnowledgeRootsRequest,
  LlmProfile,
  LlmProfileExport,
  LlmProfilePreset,
  LlmProfileTestResult,
  LoadEditableFileRequest,
  Location,
  MoveSemanticDataRequest,
  NavigateRequest,
  OneDriveAuthorizationAttempt,
  OpenDocxPreviewRequest,
  OpenPptxPreviewRequest,
  OpenStructuredViewRequest,
  Operation,
  OperationId,
  ParseKnowledgeQueryRequest,
  PlanKnowledgeSearchRequest,
  PlanSemanticExclusionRequest,
  PlanSemanticModelMigrationRequest,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  PptxPreview,
  PptxPreviewSessionRequest,
  PreviewDocumentSummaryRequest,
  PreviewRagRequest,
  PreviewSemanticEnrolmentRequest,
  RagAnswer,
  RagPreview,
  ReadDocxPreviewResourceRequest,
  ReadFileRangeRequest,
  ReadPptxPreviewPdfRequest,
  ReadStructuredJsonWindowRequest,
  ReadStructuredRowsRequest,
  RemoveApplicationDockIconRequest,
  RemoveApplicationDockIconResult,
  ResolveConflictRequest,
  ResolvedRagCitation,
  ResolveKnowledgeSourceRequest,
  ResolveRagCitationRequest,
  ResumeSemanticCleanupRequest,
  ReviewConceptCandidateRequest,
  RuntimeCapabilities,
  SaveChecksumFileRequest,
  SavedChecksumFile,
  SavedRagConversation,
  SaveEditableFileRequest,
  SaveLlmProfileRequest,
  SaveRagConversationRequest,
  ScanDiskUsageRequest,
  ScanDiskUsageResult,
  SearchInFileMatch,
  SearchInFileRequest,
  SearchInFileResult,
  SearchQuery,
  SearchStructuredRowsRequest,
  SemanticComponentCapabilities,
  SemanticComponentLifecycle,
  SemanticComponentOperation,
  SemanticComponentStatus,
  SemanticDataMoveReceipt,
  SemanticDeletionCategoryStatus,
  SemanticDiskUse,
  SemanticEnrolmentPreview,
  SemanticExclusionPlan,
  SemanticFolderStatus,
  SemanticIndexRecordCounts,
  SemanticIndexRemovalPlan,
  SemanticIndexRemovalReceipt,
  SemanticInstallationOffer,
  SemanticInstallReceipt,
  SemanticLibraryCapabilities,
  SemanticLibraryRevisionRequest,
  SemanticLibraryStatus,
  SemanticModelIdentity,
  SemanticModelMigrationPlan,
  SemanticModelMigrationProgress,
  SemanticModelProfile,
  SemanticModelSelection,
  SemanticOcrJob,
  SemanticOcrStatus,
  SemanticOcrTarget,
  SemanticProfile,
  SemanticRootStatus,
  SemanticUninstallReceipt,
  SemanticVocabulary,
  SemanticWorkerPatchResponse,
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
  StartSemanticOcrRemediationRequest,
  StructuredJsonWindow,
  StructuredRowSearch,
  StructuredRows,
  StructuredView,
  StructuredViewSessionRequest,
  StructuredViewStatus,
  SyncPlan,
  SystemLocation,
  UninstallSemanticComponentsRequest,
  Unsubscribe,
  UpdateConnectionRequest,
  UpdateSemanticEligibilityOverridesRequest,
  UpdateStructuredViewRequest,
  VerificationReport,
  VerificationResult,
  Volume,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import { defaultKnowledgeSearchOptions } from '../../models';
import { EventStreamSignalRegistry, MutableEventStreamStatus } from '../events/event-stream';
import type { FileManagerClient, NativeFileDrop } from './file-manager-client';
import {
  createGeneratedDirectory,
  GENERATED_DIRECTORY_SIZES,
  type GeneratedDirectorySize,
} from './mock-directory-generator';
import {
  buildMockKnowledgeAnswer,
  executeMockKnowledgeSearch,
  MockKnowledgeEvidenceCache,
  MockKnowledgeScopeError,
  mockKnowledgeCapabilities,
  mockKnowledgeRoots,
  mockKnowledgeRouteUnavailable,
  parseMockKnowledgeQuery,
  planMockKnowledgeSearch,
  resolveMockKnowledgeScope,
  resolveMockKnowledgeSource,
} from './mock-knowledge-search';

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
  | 'getSemanticComponentCapabilities'
  | 'getSemanticComponentStatus'
  | 'listSemanticComponentProfiles'
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
  | 'startSemanticOcrRemediation'
  | 'cancelSemanticOcrRemediation'
  | 'getSemanticLibraryCapabilities'
  | 'getSemanticLibraryStatus'
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
  | 'listLlmProfilePresets'
  | 'listLlmProfiles'
  | 'createLlmProfile'
  | 'updateLlmProfile'
  | 'deleteLlmProfile'
  | 'cloneLlmProfile'
  | 'exportLlmProfile'
  | 'activateLlmProfile'
  | 'testLlmProfile'
  | 'discoverLlmProfileModels'
  | 'discoverLlmProfileDraftModels'
  | 'previewDocumentSummary'
  | 'generateDocumentSummary'
  | 'getDocumentSummary'
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

const SEMANTIC_OPERATIONS: readonly SemanticComponentOperation[] = [
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

const MOCK_SEMANTIC_ENROLMENT_INVENTORY: Readonly<Record<string, SemanticIndexRecordCounts>> = {
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

function sameSemanticIndexCounts(
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

function mockSemanticIdentity(profile: SemanticProfile): SemanticModelIdentity {
  const suffix = {
    compactMultilingual: 'compact-multilingual',
    compactEnglish: 'compact-english',
    multilingualQuality: 'multilingual-quality',
  }[profile];
  return {
    modelId: `mock-${suffix}`,
    revision: `mock-${suffix}-revision`,
  };
}

function mockSemanticSelection(profile: SemanticProfile): SemanticModelSelection {
  return { profile, identity: mockSemanticIdentity(profile) };
}

function mockSemanticProfiles(): SemanticModelProfile[] {
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

function mockSemanticInstalledComponents(): SemanticComponentStatus['components'] {
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

function emptySemanticDiskUse(): SemanticDiskUse {
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

function installedSemanticDiskUse(): SemanticDiskUse {
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

function retainedSemanticDiskUse(): SemanticDiskUse {
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

function mockSemanticMigrationProgress(
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

function mockSemanticStatus(lifecycleName: MockSemanticLifecycle): SemanticComponentStatus {
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

function mockSemanticOffer(offerId: string, profile: SemanticProfile): SemanticInstallationOffer {
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

const MOCK_SEMANTIC_DELETION_CATEGORIES = [
  'occurrences',
  'extractedContent',
  'summaries',
  'labels',
  'orphanVectors',
  'conversationEvidencePins',
] as const;

function mockSemanticLibraryStatus(): SemanticLibraryStatus {
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

function mockSemanticOcrStatus(): SemanticOcrStatus {
  return {
    enabled: false,
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

function sameSemanticOcrTarget(left: SemanticOcrTarget, right: SemanticOcrTarget): boolean {
  return (
    left.rootId === right.rootId &&
    left.location.providerId === right.location.providerId &&
    left.location.uri === right.location.uri
  );
}

function semanticLocationContains(root: Location, candidate: Location): boolean {
  if (root.providerId !== candidate.providerId) return false;
  if (root.uri === candidate.uri) return true;
  const prefix = root.uri.endsWith('/') ? root.uri : `${root.uri}/`;
  return candidate.uri.startsWith(prefix);
}

function mockCleanupCategories(complete: boolean): SemanticDeletionCategoryStatus[] {
  return MOCK_SEMANTIC_DELETION_CATEGORIES.map((category, index) => ({
    category,
    totalItems: index + 1,
    completedItems: complete ? index + 1 : 0,
    complete,
    lastError: null,
  }));
}

/** Strictly typed controls for the deterministic in-memory frontend adapter. */
export class MockFileManagerClient implements FileManagerClient {
  readonly connection = new MutableEventStreamStatus();

  async openExternalUrl(url: string): Promise<void> {
    const opened = globalThis.open(url, '_blank', 'noopener,noreferrer');
    if (opened === null) throw new Error('The browser blocked the external link.');
    opened.opener = null;
  }

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
  private semanticStatus: SemanticComponentStatus;
  private semanticOcrStatus: SemanticOcrStatus;
  private readonly semanticOcrAutoAdvance: boolean;
  private semanticOcrJobSequence = 0;
  private readonly semanticOcrJobTargets = new Map<string, readonly SemanticOcrTarget[]>();
  private semanticOfferSequence = 0;
  private semanticIndexRemovalSequence = 0;
  private semanticMigrationSequence = 0;
  private readonly semanticOffers = new Map<string, SemanticInstallationOffer>();
  private readonly semanticEnrolmentInventory = new Map(
    Object.entries(MOCK_SEMANTIC_ENROLMENT_INVENTORY).map(([enrolmentId, counts]) => [
      enrolmentId,
      structuredClone(counts),
    ]),
  );
  private readonly semanticIndexRemovalPlans = new Map<string, SemanticIndexRemovalPlan>();
  private readonly semanticMigrationPlans = new Map<string, SemanticModelMigrationPlan>();
  private semanticLibrary = mockSemanticLibraryStatus();
  private semanticVocabularies: SemanticVocabulary[] = [];
  private semanticLibrarySequence = 0;
  private readonly semanticEnrolmentPreviews = new Map<
    string,
    PreviewSemanticEnrolmentRequest & { readonly policyRevision: number }
  >();
  private readonly semanticExclusionPlans = new Map<
    string,
    PlanSemanticExclusionRequest & { readonly rootId: string }
  >();
  private readonly finderTagsByUri = new Map<string, FinderTags>();
  private readonly spotlightCommentsByUri = new Map<string, SpotlightComment>();
  private readonly listeners = new Set<(event: BackendEvent) => void>();
  private readonly scriptedEvents: BackendEvent[] = [];
  private readonly operations = new Map<OperationId, Operation>();
  private readonly navigationHistory = new Map<string, { back: Location[]; forward: Location[] }>();
  private readonly workspaces = new Map<WorkspaceId, WorkspaceProjection>();
  private readonly connections = new Map<ConnectionId, Connection>();
  private readonly llmProfiles = new Map<string, LlmProfile>();
  private readonly documentSummaries = new Map<string, DocumentSummary>();
  private readonly ragConversations = new Map<string, SavedRagConversation>();
  private readonly knowledgeSearches = new Map<string, AbortController>();
  private readonly knowledgeAnswers = new Map<string, AbortController>();
  /** Inspected evidence sets an optional answer may be generated from. */
  private readonly knowledgeEvidence = new MockKnowledgeEvidenceCache();
  private readonly ephemeralRagConversations = new Map<string, SavedRagConversation>();
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
  private llmProfileSequence = 0;
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
  private readonly docxSessions = new Map<string, Map<string, DocxPreviewResource>>();
  private readonly pptxSessions = new Map<string, Uint8Array>();
  private readonly structuredSessions = new Map<
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

  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities> {
    return this.perform('getRuntimeCapabilities', signal, () => ({
      clipboard: false,
      extendedAttributes: true,
      finderAliases: false,
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
      semanticComponentAuthority:
        this.semanticStatus.lifecycle.state === 'unavailable' ? 'unavailable' : 'deterministicMock',
      semanticRuntimeExecutableDownload:
        this.semanticStatus.lifecycle.state === 'unavailable' ? 'unavailable' : 'simulated',
      serverAdministration: false,
      systemTrash: false,
    }));
  }

  getSemanticComponentCapabilities(signal?: AbortSignal): Promise<SemanticComponentCapabilities> {
    return this.perform('getSemanticComponentCapabilities', signal, () =>
      this.semanticStatus.lifecycle.state === 'unavailable'
        ? {
            authority: 'unavailable',
            operations: [],
            runtimeExecutableDownload: 'unavailable',
          }
        : {
            authority: 'deterministicMock',
            operations: [...SEMANTIC_OPERATIONS],
            runtimeExecutableDownload: 'simulated',
          },
    );
  }

  getSemanticOcrStatus(signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.perform('getSemanticOcrStatus', signal, () => {
      if (this.semanticOcrAutoAdvance) this.advanceSemanticOcrJobs();
      return structuredClone(this.semanticOcrStatus);
    });
  }

  setSemanticOcrConsent(enabled: boolean, signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.perform('setSemanticOcrConsent', signal, () => {
      if (enabled && this.semanticOcrStatus.availability.state !== 'available') {
        throw new MockClientError('unavailable', 'OCRmyPDF is unavailable');
      }
      this.semanticOcrStatus = {
        ...this.semanticOcrStatus,
        enabled,
        jobs: enabled
          ? this.semanticOcrStatus.jobs
          : this.semanticOcrStatus.jobs.map((job) =>
              job.state === 'queued' || job.state === 'running'
                ? { ...job, state: 'cancelled', updatedAtMs: Date.now() }
                : job,
            ),
      };
      return structuredClone(this.semanticOcrStatus);
    });
  }

  startSemanticOcrRemediation(
    request: StartSemanticOcrRemediationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticOcrJob> {
    return this.perform('startSemanticOcrRemediation', signal, () => {
      if (!this.semanticOcrStatus.enabled) {
        throw new MockClientError('disabled', 'OCR remediation is disabled');
      }
      if (this.semanticOcrStatus.availability.state !== 'available') {
        throw new MockClientError('unavailable', 'OCRmyPDF is unavailable');
      }
      const reported = this.semanticOcrStatus.reportedFiles;
      let targets: readonly SemanticOcrTarget[];
      switch (request.scope) {
        case 'oneFile':
          targets = [request.file];
          break;
        case 'selectedFiles':
          targets = request.files;
          break;
        case 'enrolledRoot':
          targets = reported.filter((target) => target.rootId === request.rootId);
          break;
        case 'allReported':
          targets = reported;
          break;
      }
      if (targets.length === 0) {
        throw new MockClientError('nothingToRemediate', 'No files currently require OCR');
      }
      if (
        targets.some(
          (target) => !reported.some((candidate) => sameSemanticOcrTarget(candidate, target)),
        )
      ) {
        throw new MockClientError(
          'unreportedTarget',
          'The requested file is not currently reported as requiring OCR',
        );
      }
      const unique = targets.filter(
        (target, index) =>
          targets.findIndex((candidate) => sameSemanticOcrTarget(candidate, target)) === index,
      );
      this.semanticOcrJobSequence += 1;
      const now = Date.now();
      const job: SemanticOcrJob = {
        id: `00000000-0000-4000-8000-${String(this.semanticOcrJobSequence).padStart(12, '0')}`,
        state: 'queued',
        createdAtMs: now,
        updatedAtMs: now,
        totalFiles: unique.length,
        processedFiles: 0,
        files: [],
        availabilityFailure: null,
      };
      this.semanticOcrJobTargets.set(job.id, structuredClone(unique));
      this.semanticOcrStatus = {
        ...this.semanticOcrStatus,
        jobs: [job, ...this.semanticOcrStatus.jobs],
      };
      return structuredClone(job);
    });
  }

  cancelSemanticOcrRemediation(jobId: string, signal?: AbortSignal): Promise<SemanticOcrJob> {
    return this.perform('cancelSemanticOcrRemediation', signal, () => {
      const job = this.semanticOcrStatus.jobs.find((candidate) => candidate.id === jobId);
      if (job === undefined) throw new MockClientError('notFound', 'OCR job was not found');
      const cancelled: SemanticOcrJob =
        job.state === 'queued' || job.state === 'running'
          ? { ...job, state: 'cancelled', updatedAtMs: Date.now() }
          : job;
      this.semanticOcrJobTargets.delete(jobId);
      this.semanticOcrStatus = {
        ...this.semanticOcrStatus,
        jobs: this.semanticOcrStatus.jobs.map((candidate) =>
          candidate.id === jobId ? cancelled : candidate,
        ),
      };
      return structuredClone(cancelled);
    });
  }

  getSemanticComponentStatus(signal?: AbortSignal): Promise<SemanticComponentStatus> {
    return this.perform('getSemanticComponentStatus', signal, () =>
      structuredClone(this.semanticStatus),
    );
  }

  listSemanticComponentProfiles(signal?: AbortSignal): Promise<SemanticModelProfile[]> {
    return this.perform('listSemanticComponentProfiles', signal, () => {
      this.requireSemanticAvailable();
      return mockSemanticProfiles();
    });
  }

  createSemanticComponentInstallationOffer(
    request: CreateSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallationOffer> {
    return this.perform('createSemanticComponentInstallationOffer', signal, () => {
      this.requireSemanticAvailable();
      this.semanticOfferSequence += 1;
      const offerId = `mock-offer-${this.semanticOfferSequence}`;
      const offer = mockSemanticOffer(offerId, request.profile);
      this.semanticOffers.set(offerId, offer);
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'offered', offerId },
      };
      return structuredClone(offer);
    });
  }

  acceptSemanticComponentInstallationOffer(
    request: AcceptSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallReceipt> {
    return this.perform('acceptSemanticComponentInstallationOffer', signal, () => {
      this.requireSemanticAvailable();
      const offer = this.semanticOffers.get(request.offerId);
      if (offer === undefined) {
        throw new MockClientError('consentRequired', 'A reviewed installation offer is required');
      }
      this.semanticOffers.delete(request.offerId);
      this.semanticStatus = {
        lifecycle: { state: 'installedEnabled' },
        dataRoot: offer.dataRoot,
        activeModel: { profile: offer.profile, identity: offer.resolvedModel },
        migration: null,
        components: mockSemanticInstalledComponents(),
        diskUse: installedSemanticDiskUse(),
      };
      return {
        installedArtifactIds: offer.components.map(({ artifactId }) => artifactId),
      };
    });
  }

  pauseSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.perform('pauseSemanticComponentIndexing', signal, () => {
      this.requireSemanticLifecycle('installedEnabled');
      this.semanticStatus = { ...this.semanticStatus, lifecycle: { state: 'paused' } };
    });
  }

  resumeSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.perform('resumeSemanticComponentIndexing', signal, () => {
      this.requireSemanticLifecycle('paused');
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'installedEnabled' },
      };
    });
  }

  createSemanticComponentIndexRemovalPlan(
    request: CreateSemanticIndexRemovalPlanRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalPlan> {
    return this.perform('createSemanticComponentIndexRemovalPlan', signal, () => {
      this.requireSemanticAvailable();
      if (!/^[A-Za-z0-9._-]+$/u.test(request.enrolmentId)) {
        throw new MockClientError('invalidEnrolment', 'The enrolment identifier is invalid');
      }
      const expected = this.semanticEnrolmentInventory.get(request.enrolmentId);
      if (expected === undefined) {
        throw new MockClientError('invalidEnrolment', 'The enrolment identifier is unknown');
      }
      this.semanticIndexRemovalSequence += 1;
      const plan: SemanticIndexRemovalPlan = {
        planId: `mock-index-removal-${this.semanticIndexRemovalSequence}`,
        enrolmentId: request.enrolmentId,
        expected: structuredClone(expected),
      };
      this.semanticIndexRemovalPlans.set(plan.planId, plan);
      return structuredClone(plan);
    });
  }

  confirmSemanticComponentIndexRemoval(
    request: ConfirmSemanticIndexRemovalRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalReceipt> {
    return this.perform('confirmSemanticComponentIndexRemoval', signal, () => {
      this.requireSemanticAvailable();
      const plan = this.semanticIndexRemovalPlans.get(request.planId);
      if (plan === undefined) {
        throw new MockClientError('indexRemoval', 'The index removal plan is stale or unknown');
      }
      this.semanticIndexRemovalPlans.delete(request.planId);
      const inventory = this.semanticEnrolmentInventory.get(plan.enrolmentId);
      if (inventory === undefined || !sameSemanticIndexCounts(inventory, plan.expected)) {
        throw new MockClientError('indexRemoval', 'The index removal plan is stale or unknown');
      }
      this.semanticEnrolmentInventory.delete(plan.enrolmentId);
      for (const [planId, candidate] of this.semanticIndexRemovalPlans) {
        if (candidate.enrolmentId === plan.enrolmentId) {
          this.semanticIndexRemovalPlans.delete(planId);
        }
      }
      return {
        enrolmentId: plan.enrolmentId,
        deleted: structuredClone(inventory),
        conversationEvidenceDeleted: true,
      };
    });
  }

  moveSemanticComponentData(
    request: MoveSemanticDataRequest,
    signal?: AbortSignal,
  ): Promise<SemanticDataMoveReceipt> {
    return this.perform('moveSemanticComponentData', signal, () => {
      this.requireSemanticAvailable();
      if (request.destination.trim().length === 0) {
        throw new MockClientError('dataMigration', 'A destination is required');
      }
      const source = this.semanticStatus.dataRoot ?? 'mock/semantic';
      this.semanticStatus = { ...this.semanticStatus, dataRoot: request.destination };
      return {
        source,
        destination: request.destination,
        verifiedBytes: this.semanticStatus.diskUse.totalBytes,
        verifiedFileCount: this.semanticStatus.components.length,
      };
    });
  }

  uninstallSemanticComponents(
    request: UninstallSemanticComponentsRequest,
    signal?: AbortSignal,
  ): Promise<SemanticUninstallReceipt> {
    return this.perform('uninstallSemanticComponents', signal, () => {
      this.requireSemanticAvailable();
      const removedComponentCount = this.semanticStatus.components.length;
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'uninstalled', indexDecision: request.indexDecision },
        components: [],
        migration: null,
        ...(request.indexDecision === 'delete'
          ? { activeModel: null, diskUse: emptySemanticDiskUse() }
          : { diskUse: retainedSemanticDiskUse() }),
      };
      return { indexDecision: request.indexDecision, removedComponentCount };
    });
  }

  installSemanticComponentWorkerPatch(
    _request: InstallSemanticWorkerPatchRequest,
    signal?: AbortSignal,
  ): Promise<SemanticWorkerPatchResponse> {
    return this.perform('installSemanticComponentWorkerPatch', signal, () => {
      this.requireSemanticAvailable();
      return { receipt: null };
    });
  }

  importSemanticComponentLocalModel(
    request: ImportSemanticLocalModelRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.perform('importSemanticComponentLocalModel', signal, () => {
      this.requireSemanticAvailable();
      if (
        request.sourcePath.trim().length === 0 ||
        /^[A-Za-z][A-Za-z0-9+.-]*:\/\//u.test(request.sourcePath.trim()) ||
        request.modelId.trim().length === 0 ||
        request.upstreamRevision.trim().length === 0 ||
        request.licenseSpdx.trim().length === 0 ||
        request.licenseNotice.trim().length === 0 ||
        request.tokenizer.trim().length === 0 ||
        request.dimensions <= 0 ||
        request.normalization == null ||
        request.runtimeComponentId.trim().length === 0 ||
        request.runtimeVersionRequirement.trim().length === 0 ||
        request.languageCoverage.length === 0 ||
        request.languageCoverage.some((language) => language.trim().length === 0) ||
        request.estimatedDiskBytes <= 0 ||
        request.estimatedRamBytes <= 0
      ) {
        throw new MockClientError(
          'invalidLocalModelMetadata',
          'Complete local model metadata is required',
        );
      }
      return this.createSemanticMigrationPlan(
        request.profile,
        {
          modelId: request.modelId,
          revision: request.upstreamRevision,
        },
        request.estimate,
      );
    });
  }

  planSemanticComponentModelMigration(
    request: PlanSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.perform('planSemanticComponentModelMigration', signal, () => {
      this.requireSemanticAvailable();
      return this.createSemanticMigrationPlan(
        request.profile,
        mockSemanticIdentity(request.profile),
        request.estimate,
      );
    });
  }

  confirmSemanticComponentModelMigration(
    request: ConfirmSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.perform('confirmSemanticComponentModelMigration', signal, () => {
      this.requireSemanticAvailable();
      const plan = this.semanticMigrationPlan(request.migrationId);
      const progress = mockSemanticMigrationProgress(
        plan.migrationId,
        0,
        plan.estimate,
        plan.target,
        null,
      );
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'migrating', progress },
        migration: progress,
      };
      return structuredClone(progress);
    });
  }

  checkpointSemanticComponentModelMigration(
    request: CheckpointSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.perform('checkpointSemanticComponentModelMigration', signal, () => {
      this.requireSemanticAvailable();
      const plan = this.semanticMigrationPlan(request.migrationId);
      const previous = this.semanticStatus.migration;
      if (
        previous == null ||
        previous.migrationId !== request.migrationId ||
        request.completedDocuments < previous.completedDocuments ||
        request.completedDocuments > plan.estimate.documents
      ) {
        throw new MockClientError('invalidMigrationProgress', 'Migration progress is invalid');
      }
      const progress = mockSemanticMigrationProgress(
        plan.migrationId,
        request.completedDocuments,
        plan.estimate,
        plan.target,
        request.resumeCursor ?? null,
      );
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'migrating', progress },
        migration: progress,
      };
      return structuredClone(progress);
    });
  }

  completeSemanticComponentModelMigration(
    request: CompleteSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelSelection> {
    return this.perform('completeSemanticComponentModelMigration', signal, () => {
      this.requireSemanticAvailable();
      const plan = this.semanticMigrationPlan(request.migrationId);
      if (
        this.semanticStatus.migration?.migrationId !== request.migrationId ||
        this.semanticStatus.migration.completedDocuments < plan.estimate.documents
      ) {
        throw new MockClientError('invalidMigrationProgress', 'Migration is not complete');
      }
      this.semanticMigrationPlans.delete(request.migrationId);
      this.semanticStatus = {
        ...this.semanticStatus,
        lifecycle: { state: 'installedEnabled' },
        activeModel: plan.target,
        migration: null,
      };
      return structuredClone(plan.target);
    });
  }

  getSemanticLibraryCapabilities(signal?: AbortSignal): Promise<SemanticLibraryCapabilities> {
    return this.perform('getSemanticLibraryCapabilities', signal, () => ({
      authority: 'deterministicMock',
      operations: [
        'viewStatus',
        'viewFolderStatus',
        'previewEnrolment',
        'enrol',
        'planExclusion',
        'confirmExclusion',
        'resumeCleanup',
        'pause',
        'resume',
        'updateEligibilityOverrides',
      ],
    }));
  }

  getSemanticLibraryStatus(signal?: AbortSignal): Promise<SemanticLibraryStatus> {
    return this.perform('getSemanticLibraryStatus', signal, () =>
      structuredClone(this.semanticLibrary),
    );
  }

  listSemanticVocabularies(signal?: AbortSignal): Promise<readonly SemanticVocabulary[]> {
    return this.perform('listSemanticVocabularies', signal, () =>
      structuredClone(this.semanticVocabularies),
    );
  }

  importSemanticVocabulary(skosJson: string, signal?: AbortSignal): Promise<SemanticVocabulary> {
    return this.perform('importSemanticVocabulary', signal, () => {
      const parsed = JSON.parse(skosJson) as {
        id: string;
        name: string;
        concepts?: SemanticVocabulary['concepts'];
      };
      const vocabulary: SemanticVocabulary = {
        id: parsed.id,
        name: parsed.name,
        concepts: parsed.concepts ?? [],
        workspaceIds: [],
        rootIds: [],
        reviewQueue: [],
        revision: 0,
      };
      this.semanticVocabularies.push(vocabulary);
      return structuredClone(vocabulary);
    });
  }

  exportSemanticVocabulary(vocabularyId: string, signal?: AbortSignal): Promise<string> {
    return this.perform('exportSemanticVocabulary', signal, () => {
      const vocabulary = this.semanticVocabularies.find(({ id }) => id === vocabularyId);
      if (!vocabulary) throw new Error('Vocabulary not found');
      return JSON.stringify({ format: 'procyon-skos-1', ...vocabulary });
    });
  }

  attachSemanticVocabulary(
    request: AttachSemanticVocabularyRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.perform('attachSemanticVocabulary', signal, () => {
      const index = this.semanticVocabularies.findIndex(({ id }) => id === request.vocabularyId);
      const current = this.semanticVocabularies[index];
      if (!current) throw new Error('Vocabulary not found');
      const updated: SemanticVocabulary = {
        ...current,
        workspaceIds: request.workspaceId
          ? [...new Set([...current.workspaceIds, request.workspaceId])]
          : current.workspaceIds,
        rootIds: request.rootId
          ? [...new Set([...current.rootIds, request.rootId])]
          : current.rootIds,
        revision: current.revision + 1,
      };
      this.semanticVocabularies[index] = updated;
      return structuredClone(updated);
    });
  }

  reviewSemanticConceptCandidate(
    request: ReviewConceptCandidateRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.perform('reviewSemanticConceptCandidate', signal, () => {
      const index = this.semanticVocabularies.findIndex(({ id }) => id === request.vocabularyId);
      const current = this.semanticVocabularies[index];
      if (!current) throw new Error('Vocabulary not found');
      const updated: SemanticVocabulary = {
        ...current,
        reviewQueue: current.reviewQueue.map((candidate) =>
          candidate.id === request.candidateId
            ? { ...candidate, status: request.action === 'reject' ? 'rejected' : 'accepted' }
            : candidate,
        ),
        revision: current.revision + 1,
      };
      this.semanticVocabularies[index] = updated;
      return structuredClone(updated);
    });
  }

  deleteSemanticVocabulary(
    vocabularyId: string,
    confirmAffected: boolean,
    signal?: AbortSignal,
  ): Promise<DeleteSemanticVocabularyImpact> {
    return this.perform('deleteSemanticVocabulary', signal, () => {
      const vocabulary = this.semanticVocabularies.find(({ id }) => id === vocabularyId);
      if (!vocabulary) throw new Error('Vocabulary not found');
      const requiresConfirmation =
        vocabulary.workspaceIds.length > 0 || vocabulary.rootIds.length > 0;
      const deleted = confirmAffected || !requiresConfirmation;
      if (deleted) {
        this.semanticVocabularies = this.semanticVocabularies.filter(
          ({ id }) => id !== vocabularyId,
        );
      }
      return {
        vocabularyId,
        affectedWorkspaceIds: vocabulary.workspaceIds,
        affectedRootIds: vocabulary.rootIds,
        requiresConfirmation,
        deleted,
      };
    });
  }

  getSemanticFolderStatus(
    request: GetSemanticFolderStatusRequest,
    signal?: AbortSignal,
  ): Promise<SemanticFolderStatus> {
    return this.perform('getSemanticFolderStatus', signal, () => {
      this.requireActiveSemanticFolder(request.workspaceId, request.location);
      const matchingExclusions = this.semanticLibrary.roots
        .flatMap((root) =>
          root.exclusions
            .filter((exclusion) => semanticLocationContains(exclusion.location, request.location))
            .map((exclusion) => ({ root, exclusion })),
        )
        .sort(
          (left, right) => left.exclusion.location.uri.length - right.exclusion.location.uri.length,
        );
      const excluded = matchingExclusions.at(-1);
      if (excluded !== undefined) {
        return {
          consent: 'excluded',
          rootId: excluded.root.id,
          exclusionId: excluded.exclusion.id,
          workspaceReferenced: excluded.root.workspaceReferences.includes(request.workspaceId),
          sourceAvailable: excluded.root.availability.state === 'available',
          unavailableReason:
            excluded.root.availability.state === 'temporarilyUnavailable'
              ? excluded.root.availability.reason
              : null,
        };
      }
      const matchingRoots = this.semanticLibrary.roots
        .filter(
          (root) =>
            root.location.providerId === request.location.providerId &&
            (root.location.uri === request.location.uri ||
              (root.recursive && semanticLocationContains(root.location, request.location))),
        )
        .sort((left, right) => left.location.uri.length - right.location.uri.length);
      const root = matchingRoots.at(-1);
      if (root === undefined) {
        return {
          consent: 'notIncluded',
          rootId: null,
          exclusionId: null,
          workspaceReferenced: false,
          sourceAvailable: true,
          unavailableReason: null,
        };
      }
      return {
        consent:
          root.location.uri === request.location.uri ? 'includedHere' : 'inheritedFromParent',
        rootId: root.id,
        exclusionId: null,
        workspaceReferenced: root.workspaceReferences.includes(request.workspaceId),
        sourceAvailable: root.availability.state === 'available',
        unavailableReason:
          root.availability.state === 'temporarilyUnavailable' ? root.availability.reason : null,
      };
    });
  }

  previewSemanticEnrolment(
    request: PreviewSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticEnrolmentPreview> {
    return this.perform('previewSemanticEnrolment', signal, () => {
      this.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.semanticLibrarySequence += 1;
      const confirmationId = `mock-enrol-confirmation-${this.semanticLibrarySequence}`;
      this.semanticEnrolmentPreviews.set(confirmationId, {
        ...structuredClone(request),
        policyRevision: this.semanticLibrary.revision,
      });
      return {
        confirmationId,
        policyRevision: this.semanticLibrary.revision,
        location: structuredClone(request.location),
        recursive: request.recursive,
        normalizedExcerptsRetainedLocally: true,
        estimate: {
          completeness: 'partial',
          estimatedFiles: 42,
          estimatedSourceBytes: 4_200,
          estimatedExtractedBytes: 1_200,
          estimatedVectorBytes: 800,
          estimatedAdditionalLocalBytes: 2_500,
          missingModelDownloadBytes: 500,
          skippedReasonCounts: [
            { reason: 'hidden', count: 1 },
            { reason: 'unsupportedMime', count: 1 },
          ],
          exceededBudgets: [],
          unavailableReason: null,
        },
      };
    });
  }

  confirmSemanticEnrolment(
    request: ConfirmSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('confirmSemanticEnrolment', signal, () => {
      this.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.requireSemanticLibraryRevision(request.policyRevision);
      const preview = this.semanticEnrolmentPreviews.get(request.confirmationId);
      if (
        preview === undefined ||
        preview.policyRevision !== request.policyRevision ||
        preview.workspaceId !== request.workspaceId ||
        preview.location.providerId !== request.location.providerId ||
        preview.location.uri !== request.location.uri
      ) {
        throw new MockClientError(
          'staleConfirmation',
          'Semantic library confirmation is stale or invalid',
        );
      }
      this.semanticEnrolmentPreviews.delete(request.confirmationId);
      const existing = this.semanticLibrary.roots.find(
        (root) =>
          root.location.providerId === request.location.providerId &&
          root.location.uri === request.location.uri,
      );
      if (existing === undefined) {
        this.semanticLibrarySequence += 1;
        this.semanticLibrary.roots.push({
          id: `00000000-0000-0000-0000-${String(this.semanticLibrarySequence).padStart(12, '0')}`,
          location: structuredClone(request.location),
          recursive: preview.recursive,
          stableIdentityVerified: true,
          workspaceReferences: [request.workspaceId],
          eligibilityOverrides: [],
          attachedVocabularyIds: [],
          eligibilityReasonCounts: [
            { reason: 'hidden', count: 1 },
            { reason: 'unsupportedMime', count: 1 },
          ],
          ocrRequiredFiles: [],
          availability: { state: 'available' },
          reconciliationGeneration: 0,
          indexedGeneration: 0,
          exclusions: [],
        });
      } else if (!existing.workspaceReferences.includes(request.workspaceId)) {
        existing.workspaceReferences.push(request.workspaceId);
      }
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
  }

  planSemanticExclusion(
    request: PlanSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticExclusionPlan> {
    return this.perform('planSemanticExclusion', signal, () => {
      this.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.requireSemanticLibraryRevision(request.policyRevision);
      const root = this.semanticRootFor(request.location);
      if (root === undefined) {
        throw new MockClientError('notEnrolled', 'The folder is not included');
      }
      if (!root.workspaceReferences.includes(request.workspaceId)) {
        throw new MockClientError('workspaceRequired', 'An active workspace/root is required');
      }
      this.semanticLibrarySequence += 1;
      const confirmationId = `mock-exclusion-confirmation-${this.semanticLibrarySequence}`;
      this.semanticExclusionPlans.set(confirmationId, {
        ...structuredClone(request),
        rootId: root.id,
      });
      return {
        confirmationId,
        policyRevision: request.policyRevision,
        rootId: root.id,
        location: structuredClone(request.location),
        categories: mockCleanupCategories(false),
      };
    });
  }

  confirmSemanticExclusion(
    request: ConfirmSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('confirmSemanticExclusion', signal, () => {
      this.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.requireSemanticLibraryRevision(request.policyRevision);
      const plan = this.semanticExclusionPlans.get(request.confirmationId);
      if (
        plan === undefined ||
        plan.policyRevision !== request.policyRevision ||
        plan.workspaceId !== request.workspaceId ||
        plan.location.providerId !== request.location.providerId ||
        plan.location.uri !== request.location.uri
      ) {
        throw new MockClientError(
          'staleConfirmation',
          'Semantic library confirmation is stale or invalid',
        );
      }
      const root = this.semanticLibrary.roots.find((candidate) => candidate.id === plan.rootId);
      if (root === undefined) throw new MockClientError('notFound', 'Semantic root not found');
      this.semanticLibrarySequence += 1;
      root.exclusions.push({
        id: `00000000-0000-0000-0001-${String(this.semanticLibrarySequence).padStart(12, '0')}`,
        location: structuredClone(request.location),
        cleanup: {
          planId: `00000000-0000-0000-0002-${String(this.semanticLibrarySequence).padStart(12, '0')}`,
          status: 'complete',
          categories: mockCleanupCategories(true),
        },
      });
      this.semanticExclusionPlans.delete(request.confirmationId);
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
  }

  resumeSemanticCleanup(
    request: ResumeSemanticCleanupRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('resumeSemanticCleanup', signal, () => {
      this.requireSemanticLibraryRevision(request.policyRevision);
      const cleanup = this.semanticLibrary.roots
        .flatMap((root) => root.exclusions)
        .map((exclusion) => exclusion.cleanup)
        .find((candidate) => candidate.planId === request.planId);
      if (cleanup === undefined) throw new MockClientError('notFound', 'Cleanup plan not found');
      cleanup.status = 'complete';
      cleanup.categories = mockCleanupCategories(true);
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
  }

  pauseSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('pauseSemanticLibrary', signal, () => {
      this.requireSemanticLibraryRevision(request.policyRevision);
      this.semanticLibrary.paused = true;
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
  }

  resumeSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('resumeSemanticLibrary', signal, () => {
      this.requireSemanticLibraryRevision(request.policyRevision);
      this.semanticLibrary.paused = false;
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
  }

  updateSemanticEligibilityOverrides(
    request: UpdateSemanticEligibilityOverridesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.perform('updateSemanticEligibilityOverrides', signal, () => {
      this.requireSemanticLibraryRevision(request.policyRevision);
      const root = this.semanticLibrary.roots.find((candidate) => candidate.id === request.rootId);
      if (root === undefined) throw new MockClientError('notFound', 'Semantic root not found');
      if (!root.workspaceReferences.includes(request.workspaceId)) {
        throw new MockClientError('workspaceRequired', 'An active workspace/root is required');
      }
      const safeReasons = new Set([
        'hidden',
        'system',
        'applicationOrPackageBundle',
        'dependencyDirectory',
        'buildDirectory',
        'cacheDirectory',
        'gitIgnored',
      ]);
      if (
        request.overrides.some(
          (override) => override.action === 'include' && !safeReasons.has(override.reason),
        )
      ) {
        throw new MockClientError(
          'unsafeEligibilityOverride',
          'This eligibility reason cannot be overridden',
        );
      }
      root.eligibilityOverrides = structuredClone(request.overrides);
      this.advanceSemanticLibraryRevision();
      return structuredClone(this.semanticLibrary);
    });
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
        finderAliases: false,
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
        semanticComponentAuthority:
          this.semanticStatus.lifecycle.state === 'unavailable'
            ? 'unavailable'
            : 'deterministicMock',
        semanticRuntimeExecutableDownload:
          this.semanticStatus.lifecycle.state === 'unavailable' ? 'unavailable' : 'simulated',
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
      let changed = false;
      for (const root of this.semanticLibrary.roots) {
        const remaining = root.workspaceReferences.filter((id) => id !== workspaceId);
        if (remaining.length !== root.workspaceReferences.length) {
          root.workspaceReferences = remaining;
          changed = true;
        }
      }
      if (changed) this.advanceSemanticLibraryRevision();
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
        case 'updateOperationCentre':
          changed = {
            ...current,
            operationCentre: command.preferences,
            revision: current.revision + 1,
          };
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

  openDocxPreview(request: OpenDocxPreviewRequest, signal?: AbortSignal): Promise<DocxPreview> {
    return this.perform('openDocxPreview', signal, () => {
      const sessionId = crypto.randomUUID();
      const resourceId = crypto.randomUUID();
      this.docxSessions.set(
        sessionId,
        new Map([
          [
            resourceId,
            {
              data: [137, 80, 78, 71, 13, 10, 26, 10],
              mediaType: 'image/png',
            },
          ],
        ]),
      );
      return {
        sessionId,
        sourceRevision: `mock:${request.location.uri}`,
        sourceBytes: 1024,
        html: '<h1>Mock document</h1><p>Content-oriented DOCX preview.</p><img src="media/image1.png" alt="Mock image">',
        resources: [
          {
            resourceId,
            source: 'media/image1.png',
            mediaType: 'image/png',
            byteLength: 8,
          },
        ],
        omittedFeatures: ['exact pagination', 'floating objects', 'headers and footers'],
      };
    });
  }

  readDocxPreviewResource(
    request: ReadDocxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreviewResource> {
    return this.perform('readDocxPreviewResource', signal, () => {
      const resource = this.docxSessions.get(request.sessionId)?.get(request.resourceId);
      if (resource === undefined) {
        throw new MockClientError('notFound', 'DOCX preview resource not found');
      }
      return structuredClone(resource);
    });
  }

  closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('closeDocxPreview', signal, () => {
      if (!this.docxSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'DOCX preview session not found');
      }
    });
  }

  openPptxPreview(request: OpenPptxPreviewRequest, signal?: AbortSignal): Promise<PptxPreview> {
    return this.perform('openPptxPreview', signal, () => {
      const sessionId = crypto.randomUUID();
      const pdf = new TextEncoder().encode(
        '%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n' +
          '2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n' +
          '3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>endobj\n' +
          '4 0 obj<</Length 0>>stream\nendstream\nendobj\ntrailer<</Root 1 0 R>>\n%%EOF\n',
      );
      this.pptxSessions.set(sessionId, pdf);
      return {
        sessionId,
        sourceRevision: `mock:${request.location.uri}`,
        sourceBytes: 2048,
        firstPagePdf: Array.from(pdf),
      };
    });
  }

  readPptxPreviewPdf(
    request: ReadPptxPreviewPdfRequest,
    signal?: AbortSignal,
  ): Promise<FileRangeChunk> {
    return this.perform('readPptxPreviewPdf', signal, () => {
      if (request.length <= 0) {
        throw new MockClientError('invalidRequest', 'length must be a positive number of bytes');
      }
      const pdf = this.pptxSessions.get(request.sessionId);
      if (pdf === undefined) {
        throw new MockClientError('notFound', 'PPTX preview session not found');
      }
      const end = Math.min(pdf.length, request.offset + request.length);
      const slice = pdf.slice(request.offset, Math.max(request.offset, end));
      return {
        data: Array.from(slice),
        offset: request.offset,
        length: slice.length,
        eof: end >= pdf.length,
        ...(request.offset === 0 ? { probablyBinary: true } : {}),
      };
    });
  }

  closePptxPreview(request: PptxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.perform('closePptxPreview', signal, () => {
      if (!this.pptxSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'PPTX preview session not found');
      }
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
        selectedSheet: 'Summary',
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
      if (request.selectedSheet != null) session.selectedSheet = request.selectedSheet;
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

  archiveSummary(
    request: ArchiveSummaryRequest,
    signal?: AbortSignal,
  ): Promise<ArchiveSummaryResult> {
    return this.perform('archiveSummary', signal, () => {
      const archiveRootUri = request.location.uri.startsWith('file://')
        ? `archive://${request.location.uri.slice('file://'.length)}!/`
        : request.location.uri;
      const rootEntries = directories[archiveRootUri];
      if (rootEntries === undefined) {
        throw new MockClientError(
          'directoryNotFound',
          `No mock archive at ${request.location.uri}`,
        );
      }
      let uncompressedSize = 0;
      let fileCount = 0;
      let directoryCount = 0;
      const stack = [archiveRootUri];
      while (stack.length > 0) {
        const uri = stack.pop() as string;
        for (const fixture of directories[uri] ?? []) {
          const entry = fixtureEntry(uri, fixture);
          if (entry.kind === 'directory') {
            directoryCount += 1;
            stack.push(entry.location.uri);
          } else {
            fileCount += 1;
            uncompressedSize += entry.size ?? 0;
          }
        }
      }
      const outerName = request.location.uri.toLowerCase();
      const format = outerName.endsWith('.7z')
        ? '7z'
        : outerName.endsWith('.rar')
          ? 'rar'
          : outerName.endsWith('.tar.gz') || outerName.endsWith('.tgz')
            ? 'tar.gz'
            : outerName.endsWith('.tar.bz2') ||
                outerName.endsWith('.tbz2') ||
                outerName.endsWith('.tbz')
              ? 'tar.bz2'
              : outerName.endsWith('.tar.xz') || outerName.endsWith('.txz')
                ? 'tar.xz'
                : outerName.endsWith('.gz')
                  ? 'gzip'
                  : outerName.endsWith('.tar')
                    ? 'tar'
                    : 'zip';
      return {
        format,
        fileCount,
        directoryCount,
        uncompressedSize,
        compressedSize: format === 'tar' ? null : Math.max(1, Math.floor(uncompressedSize / 2)),
      };
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
      const semanticQuery = request.structuredQuery?.semantic?.query;
      const contentQuery =
        request.structuredQuery?.content?.query ?? semanticQuery ?? request.contentQuery;
      const semanticMode = request.structuredQuery?.mode === 'semantic';
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
            executionMode: semanticMode ? 'semantic' : 'liveRecursive',
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
        executionMode: semanticMode ? 'semantic' : 'liveRecursive',
        ...(semanticMode
          ? {
              semanticResults: entries.map((entry, index) => ({
                entryId: entry.id,
                location: entry.location,
                score: 1 - index * 0.05,
                bestEvidence: {
                  recordId: `mock-record-${index}`,
                  sourceId: entry.id,
                  score: 1 - index * 0.05,
                  chunkKind: 'chunk',
                  excerpt: contentQuery ?? '',
                  provenanceJson: JSON.stringify({
                    kind: 'exact',
                    value: { kind: 'textLines', startLine: 1, endLine: 3 },
                  }),
                  indexedContentHash: `mock-hash-${entry.id}`,
                  generation: 1,
                  available: true,
                  stale: false,
                  generated: false,
                  sourcePosition: 0,
                },
                additionalEvidence: [],
                additionalSourceIds: [],
              })),
            }
          : {}),
        ...(semanticMode
          ? {
              semanticCoverage: {
                eligible: entries.length,
                indexed: entries.length,
                stale: 0,
                pending: 0,
                excluded: 0,
                skipped: 0,
                failed: 0,
                unavailable: 0,
                partial: false,
              },
            }
          : {}),
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

  listLlmProfilePresets(signal?: AbortSignal): Promise<LlmProfilePreset[]> {
    return this.perform('listLlmProfilePresets', signal, () => {
      const defaults: Pick<LlmProfilePreset, 'model' | 'advanced' | 'capabilities'> = {
        model: '',
        advanced: {
          contextWindow: 8192,
          maximumAnswerTokens: 1024,
          temperature: 0.2,
          timeoutSeconds: 30,
          tlsPolicy: 'requireValidCertificate' as const,
          customHeaders: {},
        },
        capabilities: ['chatCompletions', 'modelDiscovery'],
      };
      return [
        {
          ...defaults,
          name: 'Ollama',
          preset: 'ollama',
          baseUrl: 'http://127.0.0.1:11434',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'LM Studio',
          preset: 'lmStudio',
          baseUrl: 'http://127.0.0.1:1234',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'vLLM',
          preset: 'vllm',
          baseUrl: 'http://127.0.0.1:8000',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'SGLang',
          preset: 'sglang',
          baseUrl: 'http://127.0.0.1:30000',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'OMLX',
          preset: 'omlx',
          baseUrl: 'http://127.0.0.1:8080',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'OpenAI-compatible',
          preset: 'openAiCompatible',
          baseUrl: 'https://api.openai.com',
          redactFilenames: true,
        },
        {
          ...defaults,
          name: 'Azure OpenAI',
          preset: 'azureOpenAi',
          baseUrl: 'https://example.openai.azure.com',
          apiVersion: '2024-10-21',
          redactFilenames: true,
        },
      ];
    });
  }

  listLlmProfiles(signal?: AbortSignal): Promise<LlmProfile[]> {
    return this.perform('listLlmProfiles', signal, () =>
      [...this.llmProfiles.values()].map((profile) => structuredClone(profile)),
    );
  }

  createLlmProfile(request: SaveLlmProfileRequest, signal?: AbortSignal): Promise<LlmProfile> {
    return this.perform('createLlmProfile', signal, () => {
      this.llmProfileSequence += 1;
      const host = new URL(request.baseUrl).hostname.toLowerCase();
      const profile: LlmProfile = {
        ...structuredClone(request),
        id: `00000000-0000-4000-8000-${String(this.llmProfileSequence).padStart(12, '0')}`,
        hasCredential: request.credential != null,
        locality:
          host === 'localhost' || host === '127.0.0.1' || host === '::1' ? 'loopback' : 'cloud',
        consentedHost: null,
      };
      this.llmProfiles.set(profile.id, profile);
      return structuredClone(profile);
    });
  }

  updateLlmProfile(
    profileId: string,
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.perform('updateLlmProfile', signal, () => {
      const current = this.requireLlmProfile(profileId);
      const host = new URL(request.baseUrl).hostname.toLowerCase();
      const oldHost = new URL(current.baseUrl).hostname.toLowerCase();
      const profile: LlmProfile = {
        ...structuredClone(request),
        id: profileId,
        hasCredential: request.credential != null || current.hasCredential,
        locality:
          host === 'localhost' || host === '127.0.0.1' || host === '::1' ? 'loopback' : 'cloud',
        consentedHost: host === oldHost ? (current.consentedHost ?? null) : null,
      };
      this.llmProfiles.set(profileId, profile);
      return structuredClone(profile);
    });
  }

  deleteLlmProfile(
    profileId: string,
    _request: DeleteLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.perform('deleteLlmProfile', signal, () => {
      this.requireLlmProfile(profileId);
      this.llmProfiles.delete(profileId);
    });
  }

  cloneLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfile> {
    return this.perform('cloneLlmProfile', signal, () => {
      const source = this.requireLlmProfile(profileId);
      this.llmProfileSequence += 1;
      const clone: LlmProfile = {
        ...structuredClone(source),
        id: `00000000-0000-4000-8000-${String(this.llmProfileSequence).padStart(12, '0')}`,
        name: `${source.name} copy`,
        hasCredential: false,
        consentedHost: null,
      };
      this.llmProfiles.set(clone.id, clone);
      return structuredClone(clone);
    });
  }

  exportLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileExport> {
    return this.perform('exportLlmProfile', signal, () => {
      const profile = this.requireLlmProfile(profileId);
      return structuredClone({
        name: profile.name,
        preset: profile.preset,
        baseUrl: profile.baseUrl,
        deployment: profile.deployment ?? null,
        apiVersion: profile.apiVersion ?? null,
        model: profile.model,
        advanced: profile.advanced,
        capabilities: profile.capabilities,
        redactFilenames: profile.redactFilenames,
      });
    });
  }

  activateLlmProfile(
    profileId: string,
    consent: boolean,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.perform('activateLlmProfile', signal, () => {
      const profile = this.requireLlmProfile(profileId);
      if (profile.locality === 'cloud' && !consent && profile.consentedHost == null) {
        throw new MockClientError('invalidRequest', 'Cloud consent is required');
      }
      const activated: LlmProfile = {
        ...profile,
        consentedHost:
          profile.locality === 'cloud' ? new URL(profile.baseUrl).hostname.toLowerCase() : null,
      };
      this.llmProfiles.set(profileId, activated);
      return structuredClone(activated);
    });
  }

  testLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileTestResult> {
    return this.perform('testLlmProfile', signal, () => {
      const profile = this.requireLlmProfile(profileId);
      const success = profile.model.trim().length > 0;
      const availableModels =
        profile.capabilities.includes('modelDiscovery') && profile.preset !== 'azureOpenAi'
          ? [profile.model, 'model-b']
          : null;
      return {
        profileId,
        provider: profile.preset,
        locality: profile.locality,
        success,
        category: success ? null : 'modelUnavailable',
        durationMs: 1,
        modelAvailable: success,
        availableModels,
        capabilities: [...profile.capabilities],
      };
    });
  }

  discoverLlmProfileModels(profileId: string, signal?: AbortSignal): Promise<string[]> {
    return this.perform('discoverLlmProfileModels', signal, () => {
      const profile = this.requireLlmProfile(profileId);
      return profile.capabilities.includes('modelDiscovery') && profile.preset !== 'azureOpenAi'
        ? [profile.model, 'model-b']
        : [];
    });
  }

  discoverLlmProfileDraftModels(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<string[]> {
    return this.perform('discoverLlmProfileDraftModels', signal, () =>
      request.capabilities.includes('modelDiscovery') && request.preset !== 'azureOpenAi'
        ? ['model-a', 'model-b']
        : [],
    );
  }

  previewDocumentSummary(
    request: PreviewDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummaryPreview> {
    return this.perform('previewDocumentSummary', signal, () => {
      const profile =
        request.profileId == null ? undefined : this.requireLlmProfile(request.profileId);
      return {
        selectionFingerprint: `mock-summary-${request.target.entryId}`,
        representativeTokens: Math.min(request.inputTokenBudget, 72),
        keyPassages: [
          {
            label: 'S1',
            chunkId: `mock-chunk-${request.target.entryId}`,
            content: 'A representative passage from the selected document.',
            sectionPath: ['Overview'],
            provenance: '{"kind":"textLines","start_line":1,"end_line":3}',
            clusterPopulation: 1,
            weight: 1,
            structuralAnchor: true,
          },
        ],
        profile:
          profile === undefined
            ? null
            : {
                profileId: profile.id,
                profileName: profile.name,
                modelId: profile.model,
                locality: profile.locality,
              },
        reusedSelection: this.documentSummaries.has(request.target.entryId),
      };
    });
  }

  generateDocumentSummary(
    request: GenerateDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary> {
    return this.perform('generateDocumentSummary', signal, () => {
      const profile = this.requireLlmProfile(request.profileId);
      const expected = `mock-summary-${request.target.entryId}`;
      if (request.expectedSelectionFingerprint !== expected) {
        throw new MockClientError('invalidRequest', 'Document summary confirmation is stale');
      }
      const summary: DocumentSummary = {
        recordId: `mock-generated-${request.target.entryId}`,
        sourceGeneration: 1,
        profileId: profile.id,
        modelId: profile.model,
        supportingChunkIds: [`mock-chunk-${request.target.entryId}`],
        supportingWeights: [1],
        createdAtMs: Date.now(),
        brief: 'A concise representative summary.',
        full: 'A fuller representative summary grounded in the selected key passage.',
        stale: false,
      };
      this.documentSummaries.set(request.target.entryId, summary);
      return structuredClone(summary);
    });
  }

  getDocumentSummary(
    request: GetDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary | null> {
    return this.perform('getDocumentSummary', signal, () => {
      const summary = this.documentSummaries.get(request.target.entryId);
      return summary === undefined ? null : structuredClone(summary);
    });
  }

  previewRag(request: PreviewRagRequest, signal?: AbortSignal): Promise<RagPreview> {
    return this.perform('previewRag', signal, () => {
      const requestedStrategy = request.retrievalStrategy ?? 'singleQuery';
      const plannedQueries =
        requestedStrategy === 'multiQuery'
          ? [
              request.question,
              `${request.question} key facts`,
              `${request.question} supporting details`,
            ]
          : [request.question];
      return {
        appliedStrategy: requestedStrategy,
        coverage: {
          eligible: 3,
          indexed: 3,
          pending: 0,
          failed: 0,
          excluded: 0,
          stale: 0,
          unavailable: 0,
        },
        evidence: [
          {
            available: true,
            excerpt: `Mock indexed evidence relevant to "${request.question}".`,
            generated: false,
            label: 'C1',
            sourceId: 'mock-source-1',
            provenance: 'section 1',
            score: 0.91,
            sectionPath: ['Overview'],
            stale: false,
            title: 'Example indexed document',
          },
        ],
        evidenceTokens: 18,
        fallbackReason: null,
        fusionVersion: requestedStrategy === 'multiQuery' ? 'reciprocal-rank-fusion/1' : null,
        insufficient: false,
        locality: 'loopback',
        plannedQueries,
        plannerVersion: requestedStrategy === 'multiQuery' ? 'grounded-rag-query-planner/1' : null,
        profileId: request.profileId,
        profileName: 'Local mock profile',
        requestedStrategy,
        retrievalFingerprint: `mock-rag-${request.profileId}-${requestedStrategy}-${request.question}`,
        scope: structuredClone(request.scope),
      };
    });
  }

  generateRagAnswer(
    request: GenerateRagAnswerRequest,
    signal?: AbortSignal,
  ): Promise<GenerateRagAnswerResponse> {
    return this.perform('generateRagAnswer', signal, () => {
      const requestedStrategy = request.retrievalStrategy ?? 'singleQuery';
      const expectedFingerprint = `mock-rag-${request.profileId}-${requestedStrategy}-${request.question}`;
      if (request.expectedRetrievalFingerprint !== expectedFingerprint) {
        throw new MockClientError('invalidRequest', 'Ask evidence confirmation is stale');
      }
      const preview: RagPreview = {
        appliedStrategy: requestedStrategy,
        coverage: {
          eligible: 3,
          indexed: 3,
          pending: 0,
          failed: 0,
          excluded: 0,
          stale: 0,
          unavailable: 0,
        },
        evidence: [
          {
            available: true,
            excerpt: `Mock indexed evidence relevant to "${request.question}".`,
            generated: false,
            label: 'C1',
            sourceId: 'mock-source-1',
            provenance: 'section 1',
            score: 0.91,
            sectionPath: ['Overview'],
            stale: false,
            title: 'Example indexed document',
          },
        ],
        evidenceTokens: 18,
        fallbackReason: null,
        fusionVersion: requestedStrategy === 'multiQuery' ? 'reciprocal-rank-fusion/1' : null,
        insufficient: false,
        locality: 'loopback',
        plannedQueries:
          requestedStrategy === 'multiQuery'
            ? [
                request.question,
                `${request.question} key facts`,
                `${request.question} supporting details`,
              ]
            : [request.question],
        plannerVersion: requestedStrategy === 'multiQuery' ? 'grounded-rag-query-planner/1' : null,
        profileId: request.profileId,
        profileName: 'Local mock profile',
        requestedStrategy,
        retrievalFingerprint: expectedFingerprint,
        scope: structuredClone(request.scope),
      };
      const answer: RagAnswer = {
        text: 'This answer is grounded in the selected local evidence [C1].',
        citations: [
          {
            generated: false,
            label: 'C1',
            provenance: 'section 1',
            sourceId: 'mock-source-1',
            stale: false,
            unavailable: false,
          },
        ],
        modelKnowledgeAllowed: request.allowModelKnowledge,
      };
      const conversationId =
        request.conversationId ?? `rag-${this.ephemeralRagConversations.size + 1}`;
      const existing = this.ephemeralRagConversations.get(conversationId);
      if (
        existing !== undefined &&
        (existing.profileId !== request.profileId ||
          existing.scope.kind !== request.scope.kind ||
          existing.modelKnowledgeAllowed !== request.allowModelKnowledge ||
          existing.retrievalStrategy !== requestedStrategy)
      ) {
        throw new MockClientError(
          'invalidRequest',
          'Conversation profile, scope, and knowledge mode cannot change',
        );
      }
      this.ephemeralRagConversations.set(conversationId, {
        id: conversationId,
        modelKnowledgeAllowed: request.allowModelKnowledge,
        profileId: request.profileId,
        retrievalStrategy: requestedStrategy,
        scope: structuredClone(request.scope),
        storageBytes: 0,
        turns: [...(existing?.turns ?? []), { question: request.question, answer }],
      });
      return {
        conversationId,
        events: [
          { type: 'retrieval', preview },
          { type: 'token', text: answer.text },
          { type: 'done', answer },
        ],
      };
    });
  }

  saveRagConversation(
    request: SaveRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation> {
    return this.perform('saveRagConversation', signal, () => {
      const conversation = this.ephemeralRagConversations.get(request.conversationId);
      if (conversation === undefined || conversation.scope.workspaceId !== request.workspaceId) {
        throw new MockClientError('notFound', 'Ask conversation not found');
      }
      const saved = {
        ...structuredClone(conversation),
        storageBytes: JSON.stringify(conversation).length,
      };
      this.ragConversations.set(saved.id, saved);
      return structuredClone(saved);
    });
  }

  listSavedRagConversations(
    workspaceId: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation[]> {
    return this.perform('listSavedRagConversations', signal, () =>
      [...this.ragConversations.values()]
        .filter((conversation) => conversation.scope.workspaceId === workspaceId)
        .map((conversation) => structuredClone(conversation)),
    );
  }

  deleteRagConversation(
    request: DeleteRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.perform('deleteRagConversation', signal, () => {
      const conversation = this.ragConversations.get(request.conversationId);
      if (conversation === undefined) {
        throw new MockClientError('notFound', 'Ask conversation not found');
      }
      this.ragConversations.delete(request.conversationId);
    });
  }

  resolveRagCitation(
    request: ResolveRagCitationRequest,
    signal?: AbortSignal,
  ): Promise<ResolvedRagCitation> {
    return this.perform('resolveRagCitation', signal, () => {
      if (request.sourceId !== 'mock-source-1') {
        throw new MockClientError('notFound', 'Citation not found');
      }
      return {
        entryId: '11111111-1111-4111-8111-111111111111',
        location: { providerId: 'local', uri: 'file:///documents/report.txt' },
        available: true,
      };
    });
  }

  /**
   * Reports full-text retrieval, no query embeddings, and answer generation
   * only when this host has a saved generation profile (task 0207). Search
   * never depends on the answer capability.
   */
  getKnowledgeCapabilities(signal?: AbortSignal): Promise<KnowledgeCapabilities> {
    return this.perform('getKnowledgeCapabilities', signal, () =>
      mockKnowledgeCapabilities(this.llmProfiles.size > 0),
    );
  }

  listKnowledgeRoots(
    request: ListKnowledgeRootsRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeRoot[]> {
    return this.perform('listKnowledgeRoots', signal, () => {
      if (request.workspaceId.length === 0) {
        throw new MockClientError('workspaceRequired', 'Knowledge roots require a workspace');
      }
      return mockKnowledgeRoots();
    });
  }

  parseKnowledgeQuery(
    request: ParseKnowledgeQueryRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeQueryInterpretation> {
    return this.perform('parseKnowledgeQuery', signal, () => parseMockKnowledgeQuery(request.text));
  }

  planKnowledgeSearch(
    request: PlanKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchPlan> {
    return this.perform('planKnowledgeSearch', signal, () => this.knowledgePlan(request));
  }

  /**
   * Runs the deterministic no-LLM search. Cancellation is honoured both ways:
   * an aborted `signal` rejects, and a `cancelKnowledgeSearch` for the same
   * `requestId` (which is how the Tauri host cancels) makes the in-flight
   * search reject with the same `AbortError`.
   *
   * An already-aborted signal rejects before anything else is inspected, so a
   * cancelled request never reports a capability or validation failure the
   * caller did not actually wait for - the same order the desktop host uses.
   */
  async executeKnowledgeSearch(
    request: ExecuteKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchResult> {
    if (signal?.aborted === true) {
      throw new DOMException('The operation was aborted.', 'AbortError');
    }
    if (mockKnowledgeRouteUnavailable(request.mode ?? 'hybrid')) {
      throw new MockClientError('unavailable', 'Semantic retrieval is unavailable');
    }
    const controller = new AbortController();
    const forward = (): void => controller.abort();
    signal?.addEventListener('abort', forward, { once: true });
    this.knowledgeSearches.set(request.requestId, controller);
    try {
      const result = await this.perform('executeKnowledgeSearch', controller.signal, () =>
        executeMockKnowledgeSearch(
          request.requestId,
          this.knowledgePlan(request),
          this.llmProfiles.size > 0,
        ),
      );
      if (controller.signal.aborted) {
        throw new DOMException('The operation was aborted.', 'AbortError');
      }
      // Retaining the displayed set is the only bridge to an optional answer:
      // answering later reads this, never a fresh retrieval (task 0207).
      this.knowledgeEvidence.record(result.evidenceFingerprint, {
        workspaceId: request.scope.workspaceId,
        evidence: result.evidence.map((row) => structuredClone(row)),
      });
      return result;
    } finally {
      signal?.removeEventListener('abort', forward);
      this.knowledgeSearches.delete(request.requestId);
    }
  }

  /**
   * Cancellation is applied immediately rather than behind the simulated
   * latency, because a cancel that arrives after the work it cancels would
   * never be observable - the desktop host registers the same `requestId` with
   * its coordinator before the search starts.
   */
  cancelKnowledgeSearch(
    request: CancelKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    this.knowledgeSearches.get(request.requestId)?.abort();
    this.knowledgeSearches.delete(request.requestId);
    return this.perform('cancelKnowledgeSearch', signal, () => undefined);
  }

  resolveKnowledgeSource(
    request: ResolveKnowledgeSourceRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSourceLocation> {
    return this.perform('resolveKnowledgeSource', signal, () => {
      if (request.workspaceId.length === 0) {
        throw new MockClientError('workspaceRequired', 'Knowledge sources require a workspace');
      }
      const resolved = resolveMockKnowledgeSource(request.sourceId);
      if (resolved === undefined) {
        throw new MockClientError('notFound', 'Knowledge source not found');
      }
      return resolved;
    });
  }

  /**
   * Answers from an evidence set an earlier search already displayed (task
   * 0207). Nothing here retrieves: an unknown, evicted, or differently
   * authorized fingerprint is refused with `knowledgeEvidenceRefreshRequired`,
   * so the user must run Search again rather than have retrieval reappear.
   *
   * Cancellation is honoured both ways, exactly like a knowledge search: an
   * aborted `signal` rejects, and a `cancelKnowledgeAnswer` for the same
   * `requestId` (which is how the desktop host cancels) makes the in-flight
   * generation reject with the same `AbortError`.
   */
  async generateKnowledgeAnswer(
    request: GenerateKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeAnswer> {
    if (signal?.aborted === true) {
      throw new DOMException('The operation was aborted.', 'AbortError');
    }
    const controller = new AbortController();
    const forward = (): void => controller.abort();
    signal?.addEventListener('abort', forward, { once: true });
    this.knowledgeAnswers.set(request.requestId, controller);
    try {
      const answer = await this.perform('generateKnowledgeAnswer', controller.signal, () => {
        const retained = this.knowledgeEvidence.get(
          request.evidenceFingerprint,
          request.workspaceId,
        );
        if (retained === undefined) {
          throw new MockClientError(
            'knowledgeEvidenceRefreshRequired',
            'The inspected evidence set is no longer available; search again to answer.',
          );
        }
        const profile = this.llmProfiles.get(request.profileId);
        if (profile === undefined) {
          throw new MockClientError('unavailable', 'No generation profile is configured');
        }
        return buildMockKnowledgeAnswer({
          requestId: request.requestId,
          evidenceFingerprint: request.evidenceFingerprint,
          profileId: profile.id,
          profileName: profile.name,
          locality: profile.locality,
          allowModelKnowledge: request.allowModelKnowledge ?? false,
          evidence: retained.evidence,
        });
      });
      if (controller.signal.aborted) {
        throw new DOMException('The operation was aborted.', 'AbortError');
      }
      return answer;
    } finally {
      signal?.removeEventListener('abort', forward);
      this.knowledgeAnswers.delete(request.requestId);
    }
  }

  /**
   * Cancellation is applied immediately rather than behind the simulated
   * latency, mirroring {@link cancelKnowledgeSearch}: the desktop host
   * registers the same `requestId` before generation starts.
   */
  cancelKnowledgeAnswer(
    request: CancelKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    this.knowledgeAnswers.get(request.requestId)?.abort();
    this.knowledgeAnswers.delete(request.requestId);
    return this.perform('cancelKnowledgeAnswer', signal, () => undefined);
  }

  private knowledgePlan(
    request: PlanKnowledgeSearchRequest | ExecuteKnowledgeSearchRequest,
  ): KnowledgeSearchPlan {
    if ((request.draft.about ?? []).filter((subject) => subject.trim().length > 0).length === 0) {
      throw new MockClientError('invalidRequest', 'A knowledge search needs at least one subject');
    }
    return planMockKnowledgeSearch(
      request.draft,
      this.knowledgeScope(request),
      request.mode ?? 'hybrid',
      request.options ?? defaultKnowledgeSearchOptions(),
    );
  }

  /** Resolves the visible scope, refusing exactly what the backend refuses. */
  private knowledgeScope(
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

  private requireLlmProfile(profileId: string): LlmProfile {
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

  private requireActiveSemanticFolder(workspaceId: WorkspaceId, location: Location): void {
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

  private requireSemanticLibraryRevision(expected: number): void {
    if (this.semanticLibrary.revision !== expected) {
      throw new MockClientError(
        'staleRevision',
        `Semantic policy changed from revision ${expected} to ${this.semanticLibrary.revision}`,
      );
    }
  }

  private semanticRootFor(location: Location): SemanticRootStatus | undefined {
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

  private advanceSemanticOcrJobs(): void {
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

  private advanceSemanticLibraryRevision(): void {
    this.semanticLibrary.revision += 1;
    this.semanticEnrolmentPreviews.clear();
    this.semanticExclusionPlans.clear();
  }

  private requireSemanticAvailable(): void {
    if (this.semanticStatus.lifecycle.state === 'unavailable') {
      throw new MockClientError('unavailable', 'Semantic components are unavailable');
    }
  }

  private requireSemanticLifecycle(expected: SemanticComponentLifecycle['state']): void {
    this.requireSemanticAvailable();
    if (this.semanticStatus.lifecycle.state !== expected) {
      throw new MockClientError(
        'invalidLifecycle',
        `Expected semantic lifecycle ${expected}, got ${this.semanticStatus.lifecycle.state}`,
      );
    }
  }

  private createSemanticMigrationPlan(
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

  private semanticMigrationPlan(migrationId: string): SemanticModelMigrationPlan {
    const plan = this.semanticMigrationPlans.get(migrationId);
    if (plan === undefined) {
      throw new MockClientError('invalidMigrationPlan', 'The migration plan is no longer valid');
    }
    return plan;
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
