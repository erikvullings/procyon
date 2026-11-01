import type {
  AcceptSemanticInstallationOfferRequest,
  ActionDescriptor,
  ActionResult,
  AnalyzeCorpusThemesRequest,
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ArchiveCredentialRequest,
  ArchiveSummaryRequest,
  ArchiveSummaryResult,
  AttachSemanticVocabularyRequest,
  AttributeChangePreview,
  AttributeScope,
  BackendEvent,
  BeginOneDriveAuthorizationResponse,
  CalculateFolderSizeRequest,
  CalculateFolderSizeResult,
  CancelKnowledgeAnswerRequest,
  CancelKnowledgeSearchRequest,
  CheckpointSemanticModelMigrationRequest,
  ChecksumAlgorithm,
  ChecksumFile,
  ChecksumPage,
  ComparisonPage,
  CompleteSemanticModelMigrationRequest,
  ConfirmSemanticEnrolmentRequest,
  ConfirmSemanticExclusionRequest,
  ConfirmSemanticIndexRemovalRequest,
  ConfirmSemanticModelMigrationRequest,
  Connection,
  ConnectionId,
  CorpusThemeSources,
  CorpusThemeStatus,
  CorpusThemesResponse,
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
  DuplicatePage,
  EditableFile,
  EditableFileSave,
  EntryMetadata,
  EntryMetadataRequest,
  EntrySummary,
  ExecuteKnowledgeSearchRequest,
  FileDiffOptions,
  FileDiffResult,
  FileRangeChunk,
  FinderTags,
  FindLinkReplacementsRequest,
  FindLinkReplacementsResult,
  FrontendDiagnostic,
  GenerateDocumentSummaryRequest,
  GenerateKnowledgeAnswerRequest,
  GenerateRagAnswerRequest,
  GenerateRagAnswerResponse,
  GenerateSyncPlanRequest,
  GeoConversionReport,
  GeoDocument,
  GeoMessage,
  GeoMinify,
  GeoMinifyPreview,
  GeoTargetFormat,
  GetCorpusThemeSourcesRequest,
  GetCorpusThemeStatusRequest,
  GetDocumentSummaryRequest,
  GetSemanticFolderStatusRequest,
  GitFileHistoryRequest,
  GitFileHistoryResult,
  HostKeyProbe,
  ImageGpsEntry,
  ImageMetadata,
  ImportSemanticLocalModelRequest,
  InspectAttributesResult,
  InspectSplitSetResult,
  InstallSemanticWorkerPatchRequest,
  InvokeActionRequest,
  KnowledgeAnswer,
  KnowledgeCapabilities,
  KnowledgeQueryInterpretation,
  KnowledgeRoot,
  KnowledgeSearchPlan,
  KnowledgeSearchResult,
  KnowledgeSourceLocation,
  LinkDoctorPage,
  LinkOptions,
  LinkOptionsRequest,
  ListDirectoryRequest,
  ListKnowledgeRootsRequest,
  LlmModelLimits,
  LlmProfile,
  LlmProfileExport,
  LlmProfilePreset,
  LlmProfileTestResult,
  LoadEditableFileRequest,
  Location,
  MoveSemanticDataRequest,
  MusicDuplicatePage,
  NavigateRequest,
  OneDriveAuthorizationAttempt,
  OpenDocxPreviewRequest,
  OpenPptxPreviewRequest,
  OpenSqliteViewRequest,
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
  PluginPreviewResult,
  PptxPreview,
  PptxPreviewSessionRequest,
  PreviewDocumentSummaryRequest,
  PreviewRagRequest,
  PreviewSemanticEnrolmentRequest,
  QuerySqliteViewRequest,
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
  ResolveMetadataTokensRequest,
  ResolveMetadataTokensResult,
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
  SearchInFileRequest,
  SearchInFileResult,
  SearchStructuredRowsRequest,
  SemanticComponentCapabilities,
  SemanticComponentStatus,
  SemanticDataMoveReceipt,
  SemanticEnrolmentPreview,
  SemanticExclusionPlan,
  SemanticFolderStatus,
  SemanticIndexRemovalPlan,
  SemanticIndexRemovalReceipt,
  SemanticInstallationOffer,
  SemanticInstallReceipt,
  SemanticLibraryCapabilities,
  SemanticLibraryRevisionRequest,
  SemanticLibraryStatus,
  SemanticModelMigrationPlan,
  SemanticModelMigrationProgress,
  SemanticModelProfile,
  SemanticModelSelection,
  SemanticOcrJob,
  SemanticOcrStatus,
  SemanticUninstallReceipt,
  SemanticVocabulary,
  SemanticWorkerPatchResponse,
  SetPaneActivityRequest,
  Settings,
  SnapshotJobPage,
  SpotlightComment,
  SqliteQueryResult,
  SqliteView,
  SqliteViewSessionRequest,
  StartChecksumRequest,
  StartChecksumResult,
  StartComparisonRequest,
  StartComparisonResult,
  StartDuplicateScanRequest,
  StartDuplicateScanResult,
  StartLinkDoctorScanRequest,
  StartLinkDoctorScanResult,
  StartMusicDuplicateScanRequest,
  StartMusicDuplicateScanResult,
  StartOperationRequest,
  StartSearchRequest,
  StartSearchResult,
  StartSemanticOcrRemediationRequest,
  StartSnapshotJobRequest,
  StartSnapshotJobResult,
  StructuredJsonWindow,
  StructuredRowSearch,
  StructuredRows,
  StructuredView,
  StructuredViewSessionRequest,
  StructuredViewStatus,
  SyncPlan,
  SystemLocation,
  TransferConflicts,
  UninstallSemanticComponentsRequest,
  Unsubscribe,
  UpdateConnectionRequest,
  UpdateGemmaMediaRequest,
  UpdateSemanticEligibilityOverridesRequest,
  UpdateStructuredViewRequest,
  VerificationReport,
  Volume,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import type { FileManagerClient, NativeFileDrop } from './file-manager-client';
import { MockConnectionClient } from './mock-connection-client';
import { MockContext } from './mock-context';
import { MockOperationClient } from './mock-operation-client';
import { MockPluginClient } from './mock-plugin-client';
import { MockSemanticClient } from './mock-semantic-client';
import type { MockFileManagerClientOptions } from './mock-shared';
import { MockWorkspaceClient } from './mock-workspace-client';

export type {
  MockClientMethod,
  MockFileManagerClientOptions,
  MockSemanticLifecycle,
} from './mock-shared';
export { MockClientError } from './mock-shared';

export class MockFileManagerClient implements FileManagerClient {
  private readonly ctx: MockContext;
  private readonly connectionClient: MockConnectionClient;
  private readonly workspaceClient: MockWorkspaceClient;
  private readonly operationClient: MockOperationClient;
  private readonly semanticClient: MockSemanticClient;
  private readonly pluginClient: MockPluginClient;
  constructor(options?: MockFileManagerClientOptions) {
    this.ctx = new MockContext(options);
    this.connectionClient = new MockConnectionClient(this.ctx);
    this.workspaceClient = new MockWorkspaceClient(this.ctx);
    this.operationClient = new MockOperationClient(this.ctx);
    this.semanticClient = new MockSemanticClient(this.ctx);
    this.pluginClient = new MockPluginClient(this.ctx);
    this.ctx.connectionClient = this.connectionClient;
    this.ctx.workspaceClient = this.workspaceClient;
    this.ctx.operationClient = this.operationClient;
    this.ctx.semanticClient = this.semanticClient;
    this.ctx.pluginClient = this.pluginClient;
  }
  get connection() {
    return this.ctx.connection;
  }
  get supportsAppUpdates(): boolean {
    return this.ctx.supportsAppUpdates;
  }
  get supportsAppDataRemoval(): boolean {
    return this.ctx.supportsAppDataRemoval;
  }
  setWindowTitle(title: string): Promise<void> {
    return this.connectionClient.setWindowTitle(title);
  }
  openExternalUrl(url: string): Promise<void> {
    return this.connectionClient.openExternalUrl(url);
  }
  cacheArchivePassword(_request: ArchiveCredentialRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.cacheArchivePassword(_request, signal);
  }
  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities> {
    return this.connectionClient.getRuntimeCapabilities(signal);
  }
  getSemanticComponentCapabilities(signal?: AbortSignal): Promise<SemanticComponentCapabilities> {
    return this.semanticClient.getSemanticComponentCapabilities(signal);
  }
  getSemanticOcrStatus(signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.semanticClient.getSemanticOcrStatus(signal);
  }
  setSemanticOcrConsent(enabled: boolean, signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.semanticClient.setSemanticOcrConsent(enabled, signal);
  }
  setSemanticOcrReplaceOriginal(
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<SemanticOcrStatus> {
    return this.semanticClient.setSemanticOcrReplaceOriginal(enabled, signal);
  }
  startSemanticOcrRemediation(
    request: StartSemanticOcrRemediationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticOcrJob> {
    return this.semanticClient.startSemanticOcrRemediation(request, signal);
  }
  cancelSemanticOcrRemediation(jobId: string, signal?: AbortSignal): Promise<SemanticOcrJob> {
    return this.semanticClient.cancelSemanticOcrRemediation(jobId, signal);
  }
  getSemanticComponentStatus(signal?: AbortSignal): Promise<SemanticComponentStatus> {
    return this.semanticClient.getSemanticComponentStatus(signal);
  }
  listSemanticComponentProfiles(signal?: AbortSignal): Promise<SemanticModelProfile[]> {
    return this.semanticClient.listSemanticComponentProfiles(signal);
  }
  getGemmaLibrarySetup(
    signal?: AbortSignal,
  ): Promise<import('../../models/semantic-components').GemmaLibrarySetup | null> {
    return this.semanticClient.getGemmaLibrarySetup(signal);
  }
  initializeGemmaLibrary(
    _request: import('../../models/semantic-components').InitializeGemmaLibraryRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.semanticClient.initializeGemmaLibrary(_request, signal);
  }
  createSemanticComponentInstallationOffer(
    request: CreateSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallationOffer> {
    return this.semanticClient.createSemanticComponentInstallationOffer(request, signal);
  }
  acceptSemanticComponentInstallationOffer(
    request: AcceptSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallReceipt> {
    return this.semanticClient.acceptSemanticComponentInstallationOffer(request, signal);
  }
  pauseSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.semanticClient.pauseSemanticComponentIndexing(signal);
  }
  resumeSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.semanticClient.resumeSemanticComponentIndexing(signal);
  }
  createSemanticComponentIndexRemovalPlan(
    request: CreateSemanticIndexRemovalPlanRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalPlan> {
    return this.semanticClient.createSemanticComponentIndexRemovalPlan(request, signal);
  }
  confirmSemanticComponentIndexRemoval(
    request: ConfirmSemanticIndexRemovalRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalReceipt> {
    return this.semanticClient.confirmSemanticComponentIndexRemoval(request, signal);
  }
  moveSemanticComponentData(
    request: MoveSemanticDataRequest,
    signal?: AbortSignal,
  ): Promise<SemanticDataMoveReceipt> {
    return this.semanticClient.moveSemanticComponentData(request, signal);
  }
  uninstallSemanticComponents(
    request: UninstallSemanticComponentsRequest,
    signal?: AbortSignal,
  ): Promise<SemanticUninstallReceipt> {
    return this.semanticClient.uninstallSemanticComponents(request, signal);
  }
  installSemanticComponentWorkerPatch(
    _request: InstallSemanticWorkerPatchRequest,
    signal?: AbortSignal,
  ): Promise<SemanticWorkerPatchResponse> {
    return this.semanticClient.installSemanticComponentWorkerPatch(_request, signal);
  }
  importSemanticComponentLocalModel(
    request: ImportSemanticLocalModelRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.semanticClient.importSemanticComponentLocalModel(request, signal);
  }
  planSemanticComponentModelMigration(
    request: PlanSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.semanticClient.planSemanticComponentModelMigration(request, signal);
  }
  confirmSemanticComponentModelMigration(
    request: ConfirmSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.semanticClient.confirmSemanticComponentModelMigration(request, signal);
  }
  checkpointSemanticComponentModelMigration(
    request: CheckpointSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.semanticClient.checkpointSemanticComponentModelMigration(request, signal);
  }
  completeSemanticComponentModelMigration(
    request: CompleteSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelSelection> {
    return this.semanticClient.completeSemanticComponentModelMigration(request, signal);
  }
  getSemanticLibraryCapabilities(signal?: AbortSignal): Promise<SemanticLibraryCapabilities> {
    return this.semanticClient.getSemanticLibraryCapabilities(signal);
  }
  getSemanticLibraryStatus(signal?: AbortSignal): Promise<SemanticLibraryStatus> {
    return this.semanticClient.getSemanticLibraryStatus(signal);
  }
  updateGemmaMedia(
    request: UpdateGemmaMediaRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.updateGemmaMedia(request, signal);
  }
  listSemanticVocabularies(signal?: AbortSignal): Promise<readonly SemanticVocabulary[]> {
    return this.semanticClient.listSemanticVocabularies(signal);
  }
  importSemanticVocabulary(skosJson: string, signal?: AbortSignal): Promise<SemanticVocabulary> {
    return this.semanticClient.importSemanticVocabulary(skosJson, signal);
  }
  exportSemanticVocabulary(vocabularyId: string, signal?: AbortSignal): Promise<string> {
    return this.semanticClient.exportSemanticVocabulary(vocabularyId, signal);
  }
  attachSemanticVocabulary(
    request: AttachSemanticVocabularyRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.semanticClient.attachSemanticVocabulary(request, signal);
  }
  reviewSemanticConceptCandidate(
    request: ReviewConceptCandidateRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.semanticClient.reviewSemanticConceptCandidate(request, signal);
  }
  deleteSemanticVocabulary(
    vocabularyId: string,
    confirmAffected: boolean,
    signal?: AbortSignal,
  ): Promise<DeleteSemanticVocabularyImpact> {
    return this.semanticClient.deleteSemanticVocabulary(vocabularyId, confirmAffected, signal);
  }
  getSemanticFolderStatus(
    request: GetSemanticFolderStatusRequest,
    signal?: AbortSignal,
  ): Promise<SemanticFolderStatus> {
    return this.semanticClient.getSemanticFolderStatus(request, signal);
  }
  previewSemanticEnrolment(
    request: PreviewSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticEnrolmentPreview> {
    return this.semanticClient.previewSemanticEnrolment(request, signal);
  }
  confirmSemanticEnrolment(
    request: ConfirmSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.confirmSemanticEnrolment(request, signal);
  }
  planSemanticExclusion(
    request: PlanSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticExclusionPlan> {
    return this.semanticClient.planSemanticExclusion(request, signal);
  }
  confirmSemanticExclusion(
    request: ConfirmSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.confirmSemanticExclusion(request, signal);
  }
  resumeSemanticCleanup(
    request: ResumeSemanticCleanupRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.resumeSemanticCleanup(request, signal);
  }
  pauseSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.pauseSemanticLibrary(request, signal);
  }
  resumeSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.resumeSemanticLibrary(request, signal);
  }
  updateSemanticEligibilityOverrides(
    request: UpdateSemanticEligibilityOverridesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.semanticClient.updateSemanticEligibilityOverrides(request, signal);
  }
  getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]> {
    return this.connectionClient.getSystemLocations(signal);
  }
  getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult> {
    return this.connectionClient.getDiagnostics(signal);
  }
  recordFrontendDiagnostic(error: FrontendDiagnostic, signal?: AbortSignal): Promise<void> {
    return this.connectionClient.recordFrontendDiagnostic(error, signal);
  }
  getVolumes(signal?: AbortSignal): Promise<Volume[]> {
    return this.connectionClient.getVolumes(signal);
  }
  getHomeDirectory(signal?: AbortSignal): Promise<string | undefined> {
    return this.connectionClient.getHomeDirectory(signal);
  }
  startNativeDrag(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.connectionClient.startNativeDrag(_locations, signal);
  }
  writeOsClipboardFiles(_locations: readonly Location[], signal?: AbortSignal): Promise<boolean> {
    return this.connectionClient.writeOsClipboardFiles(_locations, signal);
  }
  readOsClipboardFiles(signal?: AbortSignal): Promise<Location[]> {
    return this.connectionClient.readOsClipboardFiles(signal);
  }
  showPlatformContextMenu(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.connectionClient.showPlatformContextMenu(_locations, signal);
  }
  listOpenWithApplications(
    _location: Location,
  ): Promise<import('./file-manager-client').OpenWithApplication[]> {
    return this.connectionClient.listOpenWithApplications(_location);
  }
  openWithApplication(_location: Location, _applicationPath: string): Promise<void> {
    return this.connectionClient.openWithApplication(_location, _applicationPath);
  }
  openWithAnyApplication(_location: Location): Promise<void> {
    return this.connectionClient.openWithAnyApplication(_location);
  }
  subscribeNativeFileDrops(_listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe> {
    return this.connectionClient.subscribeNativeFileDrops(_listener);
  }
  getSettings(signal?: AbortSignal): Promise<Settings> {
    return this.connectionClient.getSettings(signal);
  }
  getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined> {
    return this.workspaceClient.getFileIcon(sampleLocationUri, signal);
  }
  getThumbnail(
    locationUri: string,
    _size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    return this.workspaceClient.getThumbnail(locationUri, _size, signal);
  }
  getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined> {
    return this.workspaceClient.getFinderTags(locationUri, signal);
  }
  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags> {
    return this.workspaceClient.setFinderTags(locationUri, tags, signal);
  }
  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined> {
    return this.workspaceClient.getSpotlightComment(locationUri, signal);
  }
  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment> {
    return this.workspaceClient.setSpotlightComment(locationUri, comment, signal);
  }
  updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings> {
    return this.connectionClient.updateSettings(settings, signal);
  }
  getAppVersion(_signal?: AbortSignal): Promise<undefined> {
    return this.connectionClient.getAppVersion(_signal);
  }
  checkForAppUpdate(_signal?: AbortSignal): Promise<never> {
    return this.connectionClient.checkForAppUpdate(_signal);
  }
  removeApplicationData(): Promise<void> {
    return this.connectionClient.removeApplicationData();
  }
  installAppUpdate(): Promise<void> {
    return this.connectionClient.installAppUpdate();
  }
  listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]> {
    return this.workspaceClient.listWorkspaces(signal);
  }
  createWorkspace(
    request: CreateWorkspaceRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.workspaceClient.createWorkspace(request, signal);
  }
  startWorkspace(workspaceId?: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.workspaceClient.startWorkspace(workspaceId, signal);
  }
  getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.workspaceClient.getWorkspace(workspaceId, signal);
  }
  renameWorkspace(
    workspaceId: WorkspaceId,
    name: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.workspaceClient.renameWorkspace(workspaceId, name, expectedRevision, signal);
  }
  deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.workspaceClient.deleteWorkspace(workspaceId, expectedRevision, signal);
  }
  openWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.workspaceClient.openWorkspace(workspaceId, signal);
  }
  dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.workspaceClient.dispatchWorkspaceCommand(command, signal);
  }
  navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.workspaceClient.navigatePane(request, signal);
  }
  listDirectory(request: ListDirectoryRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.workspaceClient.listDirectory(request, signal);
  }
  listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]> {
    return this.workspaceClient.listDirectoryChildren(location, showHidden, signal);
  }
  getEntryMetadata(request: EntryMetadataRequest, signal?: AbortSignal): Promise<EntryMetadata> {
    return this.workspaceClient.getEntryMetadata(request, signal);
  }
  setPaneActivity(_request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.setPaneActivity(_request, signal);
  }
  previewGeoConversion(
    locations: readonly Location[],
    format: GeoTargetFormat,
    _csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<GeoConversionReport[]> {
    return this.workspaceClient.previewGeoConversion(locations, format, _csvWkt, signal);
  }
  previewGeoMinify(
    _location: Location,
    minify: GeoMinify,
    signal?: AbortSignal,
  ): Promise<GeoMinifyPreview> {
    return this.workspaceClient.previewGeoMinify(_location, minify, signal);
  }
  convertGeoText(
    text: string,
    format: GeoTargetFormat,
    _csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<{ content: string; extension: string; notes: GeoMessage[] }> {
    return this.workspaceClient.convertGeoText(text, format, _csvWkt, signal);
  }
  parseGeoText(text: string, signal?: AbortSignal): Promise<GeoDocument> {
    return this.workspaceClient.parseGeoText(text, signal);
  }
  readGeoFile(_location: Location, signal?: AbortSignal): Promise<GeoDocument> {
    return this.workspaceClient.readGeoFile(_location, signal);
  }
  readImageGps(locations: readonly Location[], signal?: AbortSignal): Promise<ImageGpsEntry[]> {
    return this.workspaceClient.readImageGps(locations, signal);
  }
  inspectAttributes(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectAttributesResult> {
    return this.workspaceClient.inspectAttributes(locations, signal);
  }
  diffFiles(
    left: Location,
    right: Location,
    options?: FileDiffOptions,
    signal?: AbortSignal,
  ): Promise<FileDiffResult> {
    return this.operationClient.diffFiles(left, right, options, signal);
  }
  inspectSplitSet(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectSplitSetResult> {
    return this.workspaceClient.inspectSplitSet(locations, signal);
  }
  previewAttributeChange(
    locations: readonly Location[],
    recursive: boolean,
    scope: AttributeScope,
    signal?: AbortSignal,
  ): Promise<AttributeChangePreview> {
    return this.workspaceClient.previewAttributeChange(locations, recursive, scope, signal);
  }
  readImageMetadata(_location: Location, signal?: AbortSignal): Promise<ImageMetadata> {
    return this.workspaceClient.readImageMetadata(_location, signal);
  }
  resolveMetadataTokens(
    request: ResolveMetadataTokensRequest,
    signal?: AbortSignal,
  ): Promise<ResolveMetadataTokensResult> {
    return this.workspaceClient.resolveMetadataTokens(request, signal);
  }
  mapTileUrlTemplate(): Promise<string> {
    return this.workspaceClient.mapTileUrlTemplate();
  }
  mediaUrl(location: Location, signal?: AbortSignal): Promise<string> {
    return this.workspaceClient.mediaUrl(location, signal);
  }
  readFileRange(request: ReadFileRangeRequest, signal?: AbortSignal): Promise<FileRangeChunk> {
    return this.workspaceClient.readFileRange(request, signal);
  }
  openDocxPreview(request: OpenDocxPreviewRequest, signal?: AbortSignal): Promise<DocxPreview> {
    return this.workspaceClient.openDocxPreview(request, signal);
  }
  readDocxPreviewResource(
    request: ReadDocxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreviewResource> {
    return this.workspaceClient.readDocxPreviewResource(request, signal);
  }
  closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.closeDocxPreview(request, signal);
  }
  openPptxPreview(request: OpenPptxPreviewRequest, signal?: AbortSignal): Promise<PptxPreview> {
    return this.workspaceClient.openPptxPreview(request, signal);
  }
  readPptxPreviewPdf(
    request: ReadPptxPreviewPdfRequest,
    signal?: AbortSignal,
  ): Promise<FileRangeChunk> {
    return this.workspaceClient.readPptxPreviewPdf(request, signal);
  }
  closePptxPreview(request: PptxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.closePptxPreview(request, signal);
  }
  openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    return this.workspaceClient.openStructuredView(request, signal);
  }
  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus> {
    return this.workspaceClient.getStructuredViewStatus(request, signal);
  }
  updateStructuredView(
    request: UpdateStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    return this.workspaceClient.updateStructuredView(request, signal);
  }
  readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows> {
    return this.workspaceClient.readStructuredRows(request, signal);
  }
  readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    signal?: AbortSignal,
  ): Promise<StructuredJsonWindow> {
    return this.workspaceClient.readStructuredJsonWindow(request, signal);
  }
  searchStructuredRows(
    request: SearchStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRowSearch> {
    return this.workspaceClient.searchStructuredRows(request, signal);
  }
  closeStructuredView(request: StructuredViewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.closeStructuredView(request, signal);
  }
  openSqliteView(request: OpenSqliteViewRequest, signal?: AbortSignal): Promise<SqliteView> {
    return this.workspaceClient.openSqliteView(request, signal);
  }
  querySqliteView(
    request: QuerySqliteViewRequest,
    signal?: AbortSignal,
  ): Promise<SqliteQueryResult> {
    return this.workspaceClient.querySqliteView(request, signal);
  }
  closeSqliteView(request: SqliteViewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.closeSqliteView(request, signal);
  }
  loadEditableFile(request: LoadEditableFileRequest, signal?: AbortSignal): Promise<EditableFile> {
    return this.workspaceClient.loadEditableFile(request, signal);
  }
  saveEditableFile(
    request: SaveEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFileSave> {
    return this.workspaceClient.saveEditableFile(request, signal);
  }
  searchInFile(request: SearchInFileRequest, signal?: AbortSignal): Promise<SearchInFileResult> {
    return this.workspaceClient.searchInFile(request, signal);
  }
  calculateFolderSize(
    request: CalculateFolderSizeRequest,
    signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult> {
    return this.workspaceClient.calculateFolderSize(request, signal);
  }
  archiveSummary(
    request: ArchiveSummaryRequest,
    signal?: AbortSignal,
  ): Promise<ArchiveSummaryResult> {
    return this.workspaceClient.archiveSummary(request, signal);
  }
  scanDiskUsage(request: ScanDiskUsageRequest, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.scanDiskUsage(request, signal);
  }
  cancelDiskUsage(_scanId: string, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.cancelDiskUsage(_scanId, signal);
  }
  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult> {
    return this.workspaceClient.discoverApplicationUninstallCandidates(request, signal);
  }
  removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult> {
    return this.workspaceClient.removeApplicationDockIcon(request, signal);
  }
  gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult> {
    return this.workspaceClient.gitFileHistory(request, signal);
  }
  startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation> {
    return this.operationClient.startOperation(request, signal);
  }
  resolveLinkTarget(location: Location, signal?: AbortSignal): Promise<EntrySummary> {
    return this.operationClient.resolveLinkTarget(location, signal);
  }
  getLinkOptions(request: LinkOptionsRequest, signal?: AbortSignal): Promise<LinkOptions> {
    return this.operationClient.getLinkOptions(request, signal);
  }
  findTransferConflicts(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<TransferConflicts> {
    return this.operationClient.findTransferConflicts(sources, destination, signal);
  }
  listOperations(signal?: AbortSignal): Promise<Operation[]> {
    return this.operationClient.listOperations(signal);
  }
  cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelOperation(operationId, signal);
  }
  undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation> {
    return this.operationClient.undoOperation(operationId, signal);
  }
  pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.operationClient.pauseOperation(operationId, signal);
  }
  resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.operationClient.resumeOperation(operationId, signal);
  }
  resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void> {
    return this.operationClient.resolveConflict(request, signal);
  }
  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]> {
    return this.pluginClient.listActions(signal);
  }
  openPluginPanel(
    _pluginId: PluginId,
    _actionId: string,
    _location: Location,
    _bounds: import('./file-manager-client').PluginPanelBounds,
    _theme: 'light' | 'dark',
  ): Promise<string> {
    return this.pluginClient.openPluginPanel(_pluginId, _actionId, _location, _bounds, _theme);
  }
  updatePluginPanelBounds(): Promise<void> {
    return this.pluginClient.updatePluginPanelBounds();
  }
  setPluginPanelVisible(): Promise<void> {
    return this.pluginClient.setPluginPanelVisible();
  }
  setPluginPanelTheme(): Promise<void> {
    return this.pluginClient.setPluginPanelTheme();
  }
  closePluginPanel(): Promise<void> {
    return this.pluginClient.closePluginPanel();
  }
  invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult> {
    return this.pluginClient.invokeAction(request, signal);
  }
  listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]> {
    return this.pluginClient.listPlugins(signal);
  }
  setPluginEnabled(pluginId: PluginId, enabled: boolean, signal?: AbortSignal): Promise<void> {
    return this.pluginClient.setPluginEnabled(pluginId, enabled, signal);
  }
  renderPluginPreview(
    pluginId: PluginId,
    location: Location,
    signal?: AbortSignal,
  ): Promise<PluginPreviewResult> {
    return this.pluginClient.renderPluginPreview(pluginId, location, signal);
  }
  getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]> {
    return this.pluginClient.getPluginLogs(pluginId, signal);
  }
  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string> {
    return this.pluginClient.getPluginIconThemeAsset(pluginId, assetPath, signal);
  }
  startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult> {
    return this.operationClient.startSearch(request, signal);
  }
  cancelSearch(searchId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelSearch(searchId, signal);
  }
  startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult> {
    return this.operationClient.startComparison(request, signal);
  }
  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage> {
    return this.operationClient.getComparison(comparisonId, options, signal);
  }
  cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelComparison(comparisonId, signal);
  }
  startChecksums(
    request: StartChecksumRequest,
    signal?: AbortSignal,
  ): Promise<StartChecksumResult> {
    return this.operationClient.startChecksums(request, signal);
  }
  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage> {
    return this.operationClient.getChecksums(jobId, options, signal);
  }
  cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelChecksums(jobId, signal);
  }
  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile> {
    return this.operationClient.renderChecksumFile(jobId, algorithm, signal);
  }
  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile> {
    return this.operationClient.saveChecksumFile(jobId, request, signal);
  }
  verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport> {
    return this.operationClient.verifyChecksumFile(jobId, content, signal);
  }
  startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult> {
    return this.operationClient.startDuplicateScan(request, signal);
  }
  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage> {
    return this.operationClient.getDuplicateScan(scanId, options, signal);
  }
  cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelDuplicateScan(scanId, signal);
  }
  startMusicDuplicateScan(
    request: StartMusicDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartMusicDuplicateScanResult> {
    return this.operationClient.startMusicDuplicateScan(request, signal);
  }
  getMusicDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<MusicDuplicatePage> {
    return this.operationClient.getMusicDuplicateScan(scanId, options, signal);
  }
  cancelMusicDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelMusicDuplicateScan(scanId, signal);
  }
  forgetFolderView(_location: Location, signal?: AbortSignal): Promise<void> {
    return this.operationClient.forgetFolderView(_location, signal);
  }
  startSnapshotJob(
    request: StartSnapshotJobRequest,
    signal?: AbortSignal,
  ): Promise<StartSnapshotJobResult> {
    return this.operationClient.startSnapshotJob(request, signal);
  }
  getSnapshotJob(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<SnapshotJobPage> {
    return this.operationClient.getSnapshotJob(jobId, options, signal);
  }
  cancelSnapshotJob(jobId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelSnapshotJob(jobId, signal);
  }
  startLinkDoctorScan(
    request: StartLinkDoctorScanRequest,
    signal?: AbortSignal,
  ): Promise<StartLinkDoctorScanResult> {
    return this.operationClient.startLinkDoctorScan(request, signal);
  }
  getLinkDoctorScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<LinkDoctorPage> {
    return this.operationClient.getLinkDoctorScan(scanId, options, signal);
  }
  cancelLinkDoctorScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.operationClient.cancelLinkDoctorScan(scanId, signal);
  }
  findLinkReplacements(
    request: FindLinkReplacementsRequest,
    signal?: AbortSignal,
  ): Promise<FindLinkReplacementsResult> {
    return this.operationClient.findLinkReplacements(request, signal);
  }
  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan> {
    return this.operationClient.generateSyncPlan(comparisonId, request, signal);
  }
  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult> {
    return this.operationClient.applySyncPlan(comparisonId, request, signal);
  }
  subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    return this.connectionClient.subscribe(listener);
  }
  disconnect(): void {
    this.connectionClient.disconnect();
  }
  onResynchronise(listener: () => void): Unsubscribe {
    return this.connectionClient.onResynchronise(listener);
  }
  listLlmProfilePresets(signal?: AbortSignal): Promise<LlmProfilePreset[]> {
    return this.semanticClient.listLlmProfilePresets(signal);
  }
  listLlmProfiles(signal?: AbortSignal): Promise<LlmProfile[]> {
    return this.semanticClient.listLlmProfiles(signal);
  }
  createLlmProfile(request: SaveLlmProfileRequest, signal?: AbortSignal): Promise<LlmProfile> {
    return this.semanticClient.createLlmProfile(request, signal);
  }
  updateLlmProfile(
    profileId: string,
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.semanticClient.updateLlmProfile(profileId, request, signal);
  }
  deleteLlmProfile(
    profileId: string,
    _request: DeleteLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.semanticClient.deleteLlmProfile(profileId, _request, signal);
  }
  cloneLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfile> {
    return this.semanticClient.cloneLlmProfile(profileId, signal);
  }
  exportLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileExport> {
    return this.semanticClient.exportLlmProfile(profileId, signal);
  }
  activateLlmProfile(
    profileId: string,
    consent: boolean,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.semanticClient.activateLlmProfile(profileId, consent, signal);
  }
  testLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileTestResult> {
    return this.semanticClient.testLlmProfile(profileId, signal);
  }
  discoverLlmProfileModels(profileId: string, signal?: AbortSignal): Promise<string[]> {
    return this.semanticClient.discoverLlmProfileModels(profileId, signal);
  }
  discoverLlmProfileDraftModels(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<string[]> {
    return this.semanticClient.discoverLlmProfileDraftModels(request, signal);
  }
  discoverLlmProfileDraftModelLimits(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmModelLimits> {
    return this.semanticClient.discoverLlmProfileDraftModelLimits(request, signal);
  }
  previewDocumentSummary(
    request: PreviewDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummaryPreview> {
    return this.semanticClient.previewDocumentSummary(request, signal);
  }
  generateDocumentSummary(
    request: GenerateDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary> {
    return this.semanticClient.generateDocumentSummary(request, signal);
  }
  getDocumentSummary(
    request: GetDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary | null> {
    return this.semanticClient.getDocumentSummary(request, signal);
  }
  getCorpusThemeStatus(
    _request: GetCorpusThemeStatusRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeStatus> {
    return this.semanticClient.getCorpusThemeStatus(_request, signal);
  }
  analyzeCorpusThemes(
    _request: AnalyzeCorpusThemesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemesResponse> {
    return this.semanticClient.analyzeCorpusThemes(_request, signal);
  }
  getCorpusThemeSources(
    request: GetCorpusThemeSourcesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeSources> {
    return this.semanticClient.getCorpusThemeSources(request, signal);
  }
  previewRag(request: PreviewRagRequest, signal?: AbortSignal): Promise<RagPreview> {
    return this.semanticClient.previewRag(request, signal);
  }
  generateRagAnswer(
    request: GenerateRagAnswerRequest,
    signal?: AbortSignal,
  ): Promise<GenerateRagAnswerResponse> {
    return this.semanticClient.generateRagAnswer(request, signal);
  }
  saveRagConversation(
    request: SaveRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation> {
    return this.semanticClient.saveRagConversation(request, signal);
  }
  listSavedRagConversations(
    workspaceId: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation[]> {
    return this.semanticClient.listSavedRagConversations(workspaceId, signal);
  }
  deleteRagConversation(
    request: DeleteRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.semanticClient.deleteRagConversation(request, signal);
  }
  resolveRagCitation(
    request: ResolveRagCitationRequest,
    signal?: AbortSignal,
  ): Promise<ResolvedRagCitation> {
    return this.semanticClient.resolveRagCitation(request, signal);
  }
  getKnowledgeCapabilities(signal?: AbortSignal): Promise<KnowledgeCapabilities> {
    return this.semanticClient.getKnowledgeCapabilities(signal);
  }
  listKnowledgeRoots(
    request: ListKnowledgeRootsRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeRoot[]> {
    return this.semanticClient.listKnowledgeRoots(request, signal);
  }
  parseKnowledgeQuery(
    request: ParseKnowledgeQueryRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeQueryInterpretation> {
    return this.semanticClient.parseKnowledgeQuery(request, signal);
  }
  planKnowledgeSearch(
    request: PlanKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchPlan> {
    return this.semanticClient.planKnowledgeSearch(request, signal);
  }
  executeKnowledgeSearch(
    request: ExecuteKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchResult> {
    return this.semanticClient.executeKnowledgeSearch(request, signal);
  }
  cancelKnowledgeSearch(
    request: CancelKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.semanticClient.cancelKnowledgeSearch(request, signal);
  }
  resolveKnowledgeSource(
    request: ResolveKnowledgeSourceRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSourceLocation> {
    return this.semanticClient.resolveKnowledgeSource(request, signal);
  }
  generateKnowledgeAnswer(
    request: GenerateKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeAnswer> {
    return this.semanticClient.generateKnowledgeAnswer(request, signal);
  }
  cancelKnowledgeAnswer(
    request: CancelKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.semanticClient.cancelKnowledgeAnswer(request, signal);
  }
  listConnections(signal?: AbortSignal): Promise<Connection[]> {
    return this.workspaceClient.listConnections(signal);
  }
  createConnection(request: CreateConnectionRequest, signal?: AbortSignal): Promise<Connection> {
    return this.workspaceClient.createConnection(request, signal);
  }
  getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.workspaceClient.getConnection(connectionId, signal);
  }
  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection> {
    return this.workspaceClient.updateConnection(connectionId, request, signal);
  }
  deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void> {
    return this.workspaceClient.deleteConnection(connectionId, signal);
  }
  connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.workspaceClient.connectConnection(connectionId, signal);
  }
  disconnectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.workspaceClient.disconnectConnection(connectionId, signal);
  }
  testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.workspaceClient.testConnection(connectionId, signal);
  }
  beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse> {
    return this.workspaceClient.beginOneDriveAuthorization(connectionId, signal);
  }
  getOneDriveAuthorizationAttempt(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return this.workspaceClient.getOneDriveAuthorizationAttempt(attemptId, signal);
  }
  cancelOneDriveAuthorization(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return this.workspaceClient.cancelOneDriveAuthorization(attemptId, signal);
  }
  probeSshHostKey(connectionId: ConnectionId, signal?: AbortSignal): Promise<HostKeyProbe> {
    return this.workspaceClient.probeSshHostKey(connectionId, signal);
  }
  acceptSshHostKey(
    connectionId: ConnectionId,
    _fingerprint: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.workspaceClient.acceptSshHostKey(connectionId, _fingerprint, signal);
  }
  getOperation(operationId: OperationId): Operation | undefined {
    return this.ctx.getOperation(operationId);
  }
  scriptEvents(events: readonly BackendEvent[]): void {
    this.ctx.scriptEvents(events);
  }
  emitNextEvent(): boolean {
    return this.ctx.emitNextEvent();
  }
  emit(event: BackendEvent): void {
    this.ctx.emit(event);
  }
  emitResynchronise(): void {
    this.ctx.emitResynchronise();
  }
}
