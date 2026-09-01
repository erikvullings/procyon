import type {
  ActionDescriptor,
  ActionResult,
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ArchiveCredentialRequest,
  ArchiveSummaryRequest,
  ArchiveSummaryResult,
  BackendEvent,
  BeginOneDriveAuthorizationResponse,
  CalculateFolderSizeRequest,
  CalculateFolderSizeResult,
  ChecksumAlgorithm,
  ChecksumFile,
  ChecksumPage,
  ComparisonPage,
  Connection,
  ConnectionId,
  CreateConnectionRequest,
  CreateWorkspaceRequest,
  DiagnosticsResult,
  DirectorySnapshot,
  DiscoverApplicationUninstallCandidatesRequest,
  DiscoverApplicationUninstallCandidatesResult,
  DocxPreview,
  DocxPreviewResource,
  DocxPreviewSessionRequest,
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
  OpenDocxPreviewRequest,
  OpenPptxPreviewRequest,
  OpenStructuredViewRequest,
  Operation,
  OperationId,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  PptxPreview,
  PptxPreviewResource,
  PptxPreviewSessionRequest,
  ReadDocxPreviewResourceRequest,
  ReadFileRangeRequest,
  ReadPptxPreviewResourceRequest,
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
  SearchInFileRequest,
  SearchInFileResult,
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
  Volume,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import type { EventStreamStatusObservable } from '../events/event-stream';

/**
 * Raised by an adapter method with no implementation yet for the current
 * milestone; carries the task number that will complete it (spec §12).
 */
export class NotImplementedError extends Error {
  constructor(methodName: string, taskNumber: string) {
    super(
      `${methodName} is not implemented until task ${taskNumber}; see TASKS/${taskNumber}-*.md`,
    );
    this.name = 'NotImplementedError';
  }
}

/** A native file-reference drop reported by the desktop window. */
export interface NativeFileDrop {
  readonly locations: readonly Location[];
  readonly position: { readonly x: number; readonly y: number };
}

/**
 * Transport-neutral file manager API (spec §12). Components must depend only
 * on this interface, never on `fetch`, `EventSource` or Tauri's `invoke`
 * directly (spec §3 rule 1).
 */
export interface FileManagerClient {
  readonly connection: EventStreamStatusObservable;
  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities>;
  getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]>;
  getVolumes(signal?: AbortSignal): Promise<Volume[]>;
  /** The current user's home directory as a native path, for expanding a leading `~` typed
   * into an address bar. `undefined` where the host can't determine one (e.g. networked/browser
   * hosting). */
  getHomeDirectory(signal?: AbortSignal): Promise<string | undefined>;

  /** Starts an OS file-reference drag from the desktop host. */
  startNativeDrag(locations: readonly Location[], signal?: AbortSignal): Promise<void>;

  /** Opens the native Services (macOS) or Send To (Windows) submenu for a local selection. */
  showPlatformContextMenu(locations: readonly Location[], signal?: AbortSignal): Promise<void>;

  /** Closes the desktop window (Alt+F4, task 0128). Only implemented on the Tauri host. */
  quit?(): Promise<void>;

  /** Subscribes to Finder/Explorer file drops over the desktop window. */
  subscribeNativeFileDrops(listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe>;

  getSettings(signal?: AbortSignal): Promise<Settings>;

  updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings>;

  listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]>;

  /** Runs the workspace startup lifecycle (spec §5.3.7): opens `workspaceId` if given, otherwise
   * the last-active workspace, otherwise creates a default. */
  startWorkspace(workspaceId?: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  /** Opens a new OS window on its own private, disposable workspace forked from
   * `sourceWorkspaceId`'s current shape - or the hardcoded default shape if omitted, when there is
   * no named workspace to fork from yet (ephemeral per-window workspaces spec follow-up).
   * Desktop-only, like {@link quit}: the browser/HTTP host has no window concept. */
  openWorkspaceWindow?(sourceWorkspaceId?: WorkspaceId): Promise<void>;

  /** Writes an ephemeral (per-window) workspace's current tabs/panes/layout back into
   * `targetWorkspaceId`, or - if omitted - the named workspace it was forked from, creating one
   * if it was seeded from the hardcoded default and has no source yet (ephemeral per-window
   * workspaces spec follow-up). `targetWorkspaceId` lets the workspace switcher's per-row
   * "Update" button replace any saved workspace's content with the current session's tabs,
   * keeping that workspace's own name. Returns the target named workspace, not the ephemeral
   * one. Desktop-only, like {@link openWorkspaceWindow}. */
  resyncWorkspace?(
    ephemeralWorkspaceId: WorkspaceId,
    targetWorkspaceId?: WorkspaceId,
  ): Promise<WorkspaceProjection>;

  createWorkspace(
    request: CreateWorkspaceRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  renameWorkspace(
    workspaceId: WorkspaceId,
    name: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void>;

  openWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot>;

  listDirectory(request: ListDirectoryRequest, signal?: AbortSignal): Promise<DirectorySnapshot>;

  /** Lists the immediate child directories of a location, for the directory-tree sidebar (task
   * 0139). Not bound to a pane, unlike {@link listDirectory}: expanding a tree node never
   * disturbs a pane's own in-flight listing for the same location. */
  listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]>;

  getEntryMetadata(request: EntryMetadataRequest, signal?: AbortSignal): Promise<EntryMetadata>;

  /** Marks a pane's foreground/background state (task 0109). */
  setPaneActivity(request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void>;

  cacheArchivePassword(request: ArchiveCredentialRequest, signal?: AbortSignal): Promise<void>;

  /** Lazily fetches a native PNG icon; unsupported/failure is a themed-icon fallback. */
  getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined>;

  /** Lazily fetches a downscaled JPEG preview; unsupported/failure is an icon fallback (task 0134). */
  getThumbnail(
    locationUri: string,
    size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined>;

  /** Lazily fetches an entry's Finder tags; unsupported/failure is an empty-tags fallback
   * (task 0136), mirroring {@link getThumbnail}'s per-entry lazy/cache/fallback contract. */
  getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined>;

  /** Replaces an entry's complete set of Finder tags, matching Finder's own all-at-once tag
   * editor semantics (task 0136). Throws on failure - unlike the lazy read above, a write is a
   * deliberate user action and must surface its own error rather than fail silently. */
  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags>;

  /** Reads an entry's Spotlight comment (Get Info's "Comments:" field); unsupported/failure is
   * an absent-comment fallback (task 0136). */
  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined>;

  /** Sets or clears an entry's Spotlight comment (task 0136). Throws on failure, like
   * {@link setFinderTags}. */
  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment>;

  /** Reads one bounded byte range from a file, for the in-app large file viewer (task 0088). */
  readFileRange(request: ReadFileRangeRequest, signal?: AbortSignal): Promise<FileRangeChunk>;
  openDocxPreview(request: OpenDocxPreviewRequest, signal?: AbortSignal): Promise<DocxPreview>;
  readDocxPreviewResource(
    request: ReadDocxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreviewResource>;
  closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void>;
  openPptxPreview(request: OpenPptxPreviewRequest, signal?: AbortSignal): Promise<PptxPreview>;
  readPptxPreviewResource(
    request: ReadPptxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<PptxPreviewResource>;
  closePptxPreview(request: PptxPreviewSessionRequest, signal?: AbortSignal): Promise<void>;
  openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView>;
  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus>;
  updateStructuredView(
    request: UpdateStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView>;
  readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows>;
  readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    signal?: AbortSignal,
  ): Promise<StructuredJsonWindow>;
  searchStructuredRows(
    request: SearchStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRowSearch>;
  closeStructuredView(request: StructuredViewSessionRequest, signal?: AbortSignal): Promise<void>;
  loadEditableFile(request: LoadEditableFileRequest, signal?: AbortSignal): Promise<EditableFile>;
  saveEditableFile(
    request: SaveEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFileSave>;

  /** Searches a single file's content, for the in-app large file viewer (task 0088). */
  searchInFile(request: SearchInFileRequest, signal?: AbortSignal): Promise<SearchInFileResult>;

  /** Recursively sums a directory's total size (task 0071's Total Commander-style folder-size
   * key). Aborting `signal` (e.g. because the cursor moved to a different entry) stops the walk
   * being applied - the same one-shot cancellation convention as `readFileRange`/`searchInFile`. */
  calculateFolderSize(
    request: CalculateFolderSizeRequest,
    signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult>;

  /** Computes an archive's format, entry counts, and compressed/uncompressed sizes (task 0141). */
  archiveSummary(
    request: ArchiveSummaryRequest,
    signal?: AbortSignal,
  ): Promise<ArchiveSummaryResult>;

  /** Builds a bounded hierarchical disk-usage tree for a local directory (task 0118). */
  /** Starts an event-driven disk-usage scan. Progress, completion, and failure arrive through the
   * backend event stream, so this promise only covers request acceptance. */
  scanDiskUsage(request: ScanDiskUsageRequest, signal?: AbortSignal): Promise<void>;

  /** Cancels an active disk-usage scan. */
  cancelDiskUsage(scanId: string, signal?: AbortSignal): Promise<void>;

  /** Scans a `.app` bundle's well-known related-file locations, for the uninstall review
   * checklist (task 0148). Read-only: nothing is deleted by this call, and nothing outside the
   * fixed set of well-known macOS locations is ever touched. */
  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult>;

  /** Removes a `.app` bundle's pinned Dock icon, if it has one, once the user confirms an
   * uninstall (task 0148 follow-up) - otherwise the icon is left dangling, pointing at a
   * now-trashed bundle. `removed: false` is a normal outcome (there was none to remove), not a
   * failure. */
  removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult>;

  /** Fetches a file's git commit history, for the Alt+Space metadata panel's history section
   * (task 0135). Resolves to an empty commit list (never rejects) when the file has no history
   * to show: outside a git working tree, on a non-local provider, or not yet committed. */
  gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult>;

  startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation>;

  listOperations(signal?: AbortSignal): Promise<Operation[]>;

  cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation>;

  pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void>;

  startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult>;

  cancelSearch(searchId: string, signal?: AbortSignal): Promise<void>;

  /** Starts a cancellable directory comparison (spec §16 milestone 5, task 0075). */
  startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult>;

  /** Pages through a comparison's streamed results, optionally differences-only. */
  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage>;

  cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void>;

  /** Proposes a sync plan from a comparison's current results; never mutates anything. */
  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan>;

  /** Applies a (possibly user-edited) sync plan through the operation engine. */
  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult>;

  /** Starts a cancellable checksum job over a selection (spec §18, task 0077). */
  startChecksums(request: StartChecksumRequest, signal?: AbortSignal): Promise<StartChecksumResult>;

  /** Pages through a checksum job's streamed results. */
  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage>;

  cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void>;

  /** Renders a job's results as coreutils-compatible checksum-file text. */
  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile>;

  /**
   * Writes a job's results to a checksum file at `request.destination`.
   *
   * Server-side by design: both hosts create files through the provider's
   * audited `WRITE` path rather than a host-specific save dialog (spec §35).
   */
  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile>;

  /** Verifies a job's digests against an existing checksum file's text. */
  verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport>;

  /** Starts a cancellable duplicate scan across one or more roots. */
  startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult>;

  /** Pages through a duplicate scan's grouped results. */
  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage>;

  cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void>;

  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]>;

  invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult>;

  /** Diagnostics view for troubleshooting and bug reports (spec §30). */
  getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult>;

  listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]>;

  setPluginEnabled(pluginId: PluginId, enabled: boolean, signal?: AbortSignal): Promise<void>;

  getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]>;

  /**
   * Fetches raw SVG markup for one icon-theme asset (task 0095); `assetPath` is a theme's
   * `PluginIconDefinition.iconPath` value, passed through verbatim.
   */
  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string>;

  subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe>;

  onResynchronise(listener: () => void): Unsubscribe;

  disconnect(): void;

  /** Lists every stored connection profile with its current runtime status (task 0103). */
  listConnections(signal?: AbortSignal): Promise<Connection[]>;

  createConnection(request: CreateConnectionRequest, signal?: AbortSignal): Promise<Connection>;

  getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection>;

  deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void>;

  /**
   * Attempts to connect. See the backend `fm_connections::ConnectionService`
   * for the honest scope of this operation before task 0104/0106 register a
   * real protocol dialer.
   */
  connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  disconnectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  /**
   * Starts OneDrive authorization and opens the returned Microsoft URL in the
   * host's system browser. OAuth codes and tokens remain backend-owned.
   */
  beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse>;

  getOneDriveAuthorizationAttempt(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt>;

  cancelOneDriveAuthorization(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt>;

  /**
   * Probes an SSH connection's currently presented host key without
   * authenticating (task 0104, spec §6.4) - lets a caller decide whether to
   * accept a never-seen or changed key before `connect`/`test` report
   * `hostKeyUnverified`/`hostKeyMismatch` via the connection's `status`.
   */
  probeSshHostKey(connectionId: ConnectionId, signal?: AbortSignal): Promise<HostKeyProbe>;

  /**
   * Accepts (persists) a host-key fingerprint for an SSH connection (task
   * 0104, spec §6.4). Never call this with a fingerprint the caller has not
   * shown the user for confirmation - the backend re-probes the host before
   * persisting, but this is the only path that ever writes to the
   * known-hosts store.
   */
  acceptSshHostKey(
    connectionId: ConnectionId,
    fingerprint: string,
    signal?: AbortSignal,
  ): Promise<void>;
}
