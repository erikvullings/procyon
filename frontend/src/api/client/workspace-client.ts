import type {
  ArchiveCredentialRequest,
  ArchiveSummaryRequest,
  ArchiveSummaryResult,
  AttributeChangePreview,
  AttributeScope,
  BeginOneDriveAuthorizationResponse,
  CalculateFolderSizeRequest,
  CalculateFolderSizeResult,
  Connection,
  ConnectionId,
  CreateConnectionRequest,
  CreateWorkspaceRequest,
  CsvGeoOptions,
  DirectorySnapshot,
  DiscoverApplicationUninstallCandidatesRequest,
  DiscoverApplicationUninstallCandidatesResult,
  DocxPreview,
  DocxPreviewResource,
  DocxPreviewSessionRequest,
  EditableFile,
  EditableFileSave,
  EntryMetadata,
  EntryMetadataRequest,
  EntrySummary,
  FileRangeChunk,
  FinderTags,
  GeoConversionReport,
  GeoDocument,
  GeoMessage,
  GeoMinify,
  GeoMinifyPreview,
  GeoTargetFormat,
  GitFileHistoryRequest,
  GitFileHistoryResult,
  HostKeyProbe,
  ImageGpsEntry,
  ImageMetadata,
  InspectAttributesResult,
  InspectSplitSetResult,
  ListDirectoryRequest,
  LoadEditableFileRequest,
  Location,
  NavigateRequest,
  OneDriveAuthorizationAttempt,
  OpenDocxPreviewRequest,
  OpenPptxPreviewRequest,
  OpenSqliteViewRequest,
  OpenStructuredViewRequest,
  PptxPreview,
  PptxPreviewSessionRequest,
  QuerySqliteViewRequest,
  ReadDocxPreviewResourceRequest,
  ReadFileRangeRequest,
  ReadPptxPreviewPdfRequest,
  ReadStructuredJsonWindowRequest,
  ReadStructuredRowsRequest,
  RemoveApplicationDockIconRequest,
  RemoveApplicationDockIconResult,
  ResolveMetadataTokensRequest,
  ResolveMetadataTokensResult,
  SaveEditableFileRequest,
  ScanDiskUsageRequest,
  SearchInFileRequest,
  SearchInFileResult,
  SearchStructuredRowsRequest,
  SetPaneActivityRequest,
  SpotlightComment,
  SqliteQueryResult,
  SqliteView,
  SqliteViewSessionRequest,
  StructuredJsonWindow,
  StructuredRowSearch,
  StructuredRows,
  StructuredView,
  StructuredViewSessionRequest,
  StructuredViewStatus,
  UpdateConnectionRequest,
  UpdateStructuredViewRequest,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';

/**
 * Workspace lifecycle, navigation, previews, metadata and connected locations.
 */
export interface WorkspaceClient {
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

  /** Computes an archive's format, entry counts, and compressed/uncompressed sizes (task 0141). */
  archiveSummary(
    request: ArchiveSummaryRequest,
    signal?: AbortSignal,
  ): Promise<ArchiveSummaryResult>;

  /**
   * Starts OneDrive authorization and opens the returned Microsoft URL in the
   * host's system browser. OAuth codes and tokens remain backend-owned.
   */
  beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse>;

  cacheArchivePassword(request: ArchiveCredentialRequest, signal?: AbortSignal): Promise<void>;

  /** Recursively sums a directory's total size (task 0071's Total Commander-style folder-size
   * key). Aborting `signal` (e.g. because the cursor moved to a different entry) stops the walk
   * being applied - the same one-shot cancellation convention as `readFileRange`/`searchInFile`. */
  calculateFolderSize(
    request: CalculateFolderSizeRequest,
    signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult>;

  /** Cancels an active disk-usage scan. */
  cancelDiskUsage(scanId: string, signal?: AbortSignal): Promise<void>;

  cancelOneDriveAuthorization(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt>;

  closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void>;

  closePptxPreview(request: PptxPreviewSessionRequest, signal?: AbortSignal): Promise<void>;

  closeSqliteView(request: SqliteViewSessionRequest, signal?: AbortSignal): Promise<void>;

  closeStructuredView(request: StructuredViewSessionRequest, signal?: AbortSignal): Promise<void>;

  /**
   * Attempts to connect. See the backend `fm_connections::ConnectionService`
   * for the honest scope of this operation before task 0104/0106 register a
   * real protocol dialer.
   */
  connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  /** Converts GeoJSON text (an editor's content) into a text format (task 0281). */
  convertGeoText(
    text: string,
    format: GeoTargetFormat,
    csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<{ content: string; extension: string; notes: GeoMessage[] }>;

  createConnection(request: CreateConnectionRequest, signal?: AbortSignal): Promise<Connection>;

  createWorkspace(
    request: CreateWorkspaceRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void>;

  deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void>;

  disconnectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  /** Scans a `.app` bundle's well-known related-file locations, for the uninstall review
   * checklist (task 0148). Read-only: nothing is deleted by this call, and nothing outside the
   * fixed set of well-known macOS locations is ever touched. */
  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult>;

  dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  getEntryMetadata(request: EntryMetadataRequest, signal?: AbortSignal): Promise<EntryMetadata>;

  /** Lazily fetches a native PNG icon; unsupported/failure is a themed-icon fallback. */
  getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined>;

  /** Lazily fetches an entry's Finder tags; unsupported/failure is an empty-tags fallback
   * (task 0136), mirroring {@link getThumbnail}'s per-entry lazy/cache/fallback contract. */
  getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined>;

  getOneDriveAuthorizationAttempt(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt>;

  /** Reads an entry's Spotlight comment (Get Info's "Comments:" field); unsupported/failure is
   * an absent-comment fallback (task 0136). */
  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined>;

  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus>;

  /** Lazily fetches a downscaled JPEG preview; unsupported/failure is an icon fallback (task 0134). */
  getThumbnail(
    locationUri: string,
    size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined>;

  getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  /** Fetches a file's git commit history, for the Alt+Space metadata panel's history section
   * (task 0135). Resolves to an empty commit list (never rejects) when the file has no history
   * to show: outside a git working tree, on a non-local provider, or not yet committed. */
  gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult>;

  /**
   * Reads which permission, ownership and attribute edits the selection supports (per provider and
   * platform) and each entry's current state (task 0258).
   */
  inspectAttributes(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectAttributesResult>;

  /**
   * Detects the split set (`name.001`, `name.002`, …) a selection of part files belongs to, whether
   * a manifest sits next to it, and why it cannot be joined when it cannot (task 0257).
   */
  inspectSplitSet(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectSplitSetResult>;

  /** Lists every stored connection profile with its current runtime status (task 0103). */
  listConnections(signal?: AbortSignal): Promise<Connection[]>;

  listDirectory(request: ListDirectoryRequest, signal?: AbortSignal): Promise<DirectorySnapshot>;

  /** Lists the immediate child directories of a location, for the directory-tree sidebar (task
   * 0139). Not bound to a pane, unlike {@link listDirectory}: expanding a tree node never
   * disturbs a pane's own in-flight listing for the same location. */
  listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]>;

  listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]>;

  loadEditableFile(request: LoadEditableFileRequest, signal?: AbortSignal): Promise<EditableFile>;

  /**
   * URL template (`{z}/{x}/{y}` placeholders) of the backend tile proxy for the configured raster
   * basemap. The web view loads tiles from the backend and never contacts the provider (task 0275).
   */
  mapTileUrlTemplate(): Promise<string>;

  /**
   * Returns a URL an `<audio>`/`<video>` element can load with seeking (task 0260): HTTP `Range`
   * requests against the Axum host, the `procyon-media` scheme on desktop. The backend streams
   * bounded byte ranges, so large and remote files never load into memory whole.
   */
  mediaUrl(location: Location, signal?: AbortSignal): Promise<string>;

  navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot>;

  openDocxPreview(request: OpenDocxPreviewRequest, signal?: AbortSignal): Promise<DocxPreview>;

  openPptxPreview(request: OpenPptxPreviewRequest, signal?: AbortSignal): Promise<PptxPreview>;

  /** Opens a read-only SQLite database session (task 0251): schema plus later paged queries. */
  openSqliteView(request: OpenSqliteViewRequest, signal?: AbortSignal): Promise<SqliteView>;

  openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView>;

  openWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  /** Parses GeoJSON text for live validation while editing (task 0278); never throws on bad text. */
  parseGeoText(text: string, signal?: AbortSignal): Promise<GeoDocument>;

  /** Counts the entries a `changeAttributes` operation would touch before it is applied (task 0258). */
  previewAttributeChange(
    locations: readonly Location[],
    recursive: boolean,
    scope: AttributeScope,
    signal?: AbortSignal,
  ): Promise<AttributeChangePreview>;

  /** Reports what converting geographic files to `format` would lose, per file (task 0281). */
  previewGeoConversion(
    locations: readonly Location[],
    format: GeoTargetFormat,
    csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<GeoConversionReport[]>;

  /** Estimates what a minification would do to a file, without writing anything (task 0282). */
  previewGeoMinify(
    location: Location,
    minify: GeoMinify,
    signal?: AbortSignal,
  ): Promise<GeoMinifyPreview>;

  /**
   * Probes an SSH connection's currently presented host key without
   * authenticating (task 0104, spec §6.4) - lets a caller decide whether to
   * accept a never-seen or changed key before `connect`/`test` report
   * `hostKeyUnverified`/`hostKeyMismatch` via the connection's `status`.
   */
  probeSshHostKey(connectionId: ConnectionId, signal?: AbortSignal): Promise<HostKeyProbe>;

  /** Reads one page of a table/view (with search, filters, sort) or of a read-only SQL result. */
  querySqliteView(
    request: QuerySqliteViewRequest,
    signal?: AbortSignal,
  ): Promise<SqliteQueryResult>;

  readDocxPreviewResource(
    request: ReadDocxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreviewResource>;

  /** Reads one bounded byte range from a file, for the in-app large file viewer (task 0088). */
  readFileRange(request: ReadFileRangeRequest, signal?: AbortSignal): Promise<FileRangeChunk>;

  /** Reads a GeoJSON, GPX, KML or KMZ file into GeoJSON features (tasks 0277-0280). */
  readGeoFile(location: Location, signal?: AbortSignal, csv?: CsvGeoOptions): Promise<GeoDocument>;

  /**
   * Reads GPS position and capture time of up to 100 images from their leading bytes only
   * (task 0276); an unreadable image is flagged instead of failing the batch.
   */
  readImageGps(locations: readonly Location[], signal?: AbortSignal): Promise<ImageGpsEntry[]>;

  /**
   * Reads every EXIF, GPS, IPTC, XMP and ICC tag of an image from its leading bytes (task 0273).
   * Works for local, archive and remote entries because the backend reads through the VFS.
   */
  readImageMetadata(location: Location, signal?: AbortSignal): Promise<ImageMetadata>;

  readPptxPreviewPdf(
    request: ReadPptxPreviewPdfRequest,
    signal?: AbortSignal,
  ): Promise<FileRangeChunk>;

  readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    signal?: AbortSignal,
  ): Promise<StructuredJsonWindow>;

  readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows>;

  /** Removes a `.app` bundle's pinned Dock icon, if it has one, once the user confirms an
   * uninstall (task 0148 follow-up) - otherwise the icon is left dangling, pointing at a
   * now-trashed bundle. `removed: false` is a normal outcome (there was none to remove), not a
   * failure. */
  removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult>;

  renameWorkspace(
    workspaceId: WorkspaceId,
    name: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection>;

  /**
   * Resolves audio-tag and EXIF tokens for multi-rename patterns (task 0267). The backend
   * resolves and sanitises the values so both hosts produce identical names.
   */
  resolveMetadataTokens(
    request: ResolveMetadataTokensRequest,
    signal?: AbortSignal,
  ): Promise<ResolveMetadataTokensResult>;

  saveEditableFile(
    request: SaveEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFileSave>;

  /** Builds a bounded hierarchical disk-usage tree for a local directory (task 0118). */
  /** Starts an event-driven disk-usage scan. Progress, completion, and failure arrive through the
   * backend event stream, so this promise only covers request acceptance. */
  scanDiskUsage(request: ScanDiskUsageRequest, signal?: AbortSignal): Promise<void>;

  /** Searches a single file's content, for the in-app large file viewer (task 0088). */
  searchInFile(request: SearchInFileRequest, signal?: AbortSignal): Promise<SearchInFileResult>;

  searchStructuredRows(
    request: SearchStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRowSearch>;

  /** Replaces an entry's complete set of Finder tags, matching Finder's own all-at-once tag
   * editor semantics (task 0136). Throws on failure - unlike the lazy read above, a write is a
   * deliberate user action and must surface its own error rather than fail silently. */
  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags>;

  /** Marks a pane's foreground/background state (task 0109). */
  setPaneActivity(request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void>;

  /** Sets or clears an entry's Spotlight comment (task 0136). Throws on failure, like
   * {@link setFinderTags}. */
  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment>;

  /** Runs the workspace startup lifecycle (spec §5.3.7): opens `workspaceId` if given, otherwise
   * the last-active workspace, otherwise creates a default. */
  startWorkspace(workspaceId?: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection>;

  testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection>;

  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection>;

  updateStructuredView(
    request: UpdateStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView>;
}
