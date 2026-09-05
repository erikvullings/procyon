import type {
  AcceptSemanticInstallationOfferRequest,
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
  CreateConnectionRequest,
  CreateSemanticIndexRemovalPlanRequest,
  CreateSemanticInstallationOfferRequest,
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
  Location as FileLocation,
  FileRangeChunk,
  FinderTags,
  GenerateSyncPlanRequest,
  GetSemanticFolderStatusRequest,
  GitFileHistoryRequest,
  GitFileHistoryResult,
  HostKeyProbe,
  ImportSemanticLocalModelRequest,
  InstallSemanticWorkerPatchRequest,
  InvokeActionRequest,
  ListDirectoryRequest,
  LoadEditableFileRequest,
  MoveSemanticDataRequest,
  NavigateRequest,
  OneDriveAuthorizationAttempt,
  OpenDocxPreviewRequest,
  OpenPptxPreviewRequest,
  OpenStructuredViewRequest,
  Operation,
  OperationId,
  PlanSemanticExclusionRequest,
  PlanSemanticModelMigrationRequest,
  PluginDescriptor,
  PluginIconTheme,
  PluginId,
  PluginLogEntry,
  PptxPreview,
  PptxPreviewSessionRequest,
  PreviewSemanticEnrolmentRequest,
  ReadDocxPreviewResourceRequest,
  ReadFileRangeRequest,
  ReadPptxPreviewPdfRequest,
  ReadStructuredJsonWindowRequest,
  ReadStructuredRowsRequest,
  RemoveApplicationDockIconRequest,
  RemoveApplicationDockIconResult,
  ResolveConflictRequest,
  ResumeSemanticCleanupRequest,
  RuntimeCapabilities,
  SaveChecksumFileRequest,
  SavedChecksumFile,
  SaveEditableFileRequest,
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
  SemanticUninstallReceipt,
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
  Volume,
  WorkspaceCommand,
  WorkspaceId,
  WorkspaceProjection,
  WorkspaceSummary,
} from '../../models';
import {
  checksumPageFromDto,
  duplicatePageFromDto,
  verificationReportFromDto,
} from '../../models/checksum';
import { syncPlanItemToDto } from '../../models/comparison';
import { entryMetadataFromDto, entrySummaryFromDto } from '../../models/entry';
import { comparisonPageFromDto, syncPlanFromDto } from '../../models/requests';
import { directorySnapshotFromDto } from '../../models/snapshot';
import { workspaceProjectionFromDto } from '../../models/workspace';
import { SseEventStream } from '../events/sse-event-stream';
import {
  invokeAction as requestActionInvocation,
  listActions as requestActions,
  removeApplicationDockIcon as requestApplicationDockIconRemoval,
  discoverApplicationUninstallCandidates as requestApplicationUninstallDiscovery,
  cacheArchivePassword as requestArchivePasswordCache,
  archiveSummary as requestArchiveSummary,
  cancelChecksums as requestChecksumCancel,
  renderChecksumFile as requestChecksumFileRender,
  saveChecksumFile as requestChecksumFileSave,
  verifyChecksumFile as requestChecksumFileVerify,
  getChecksums as requestChecksumGet,
  startChecksums as requestChecksumStart,
  cancelComparison as requestComparisonCancel,
  getComparison as requestComparisonGet,
  startComparison as requestComparisonStart,
  resolveOperationConflict as requestConflictResolution,
  getConnection as requestConnection,
  connectConnection as requestConnectionConnect,
  createConnection as requestConnectionCreation,
  deleteConnection as requestConnectionDeletion,
  disconnectConnection as requestConnectionDisconnect,
  listConnections as requestConnections,
  testConnection as requestConnectionTest,
  updateConnection as requestConnectionUpdate,
  getDiagnostics as requestDiagnostics,
  listDirectory as requestDirectory,
  listDirectoryChildren as requestDirectoryChildren,
  cancelDiskUsage as requestDiskUsageCancel,
  scanDiskUsage as requestDiskUsageScan,
  closeDocxPreview as requestDocxPreviewClose,
  openDocxPreview as requestDocxPreviewOpen,
  readDocxPreviewResource as requestDocxPreviewResource,
  cancelDuplicateScan as requestDuplicateScanCancel,
  getDuplicateScan as requestDuplicateScanGet,
  startDuplicateScan as requestDuplicateScanStart,
  getEntryMetadata as requestEntryMetadata,
  getFileIcon as requestFileIcon,
  getFinderTags as requestFinderTags,
  setFinderTags as requestFinderTagsUpdate,
  calculateFolderSize as requestFolderSizeCalculation,
  getFileGitHistory as requestGitFileHistory,
  loadEditableFile as requestLoadEditableFile,
  navigatePane as requestNavigation,
  getOneDriveAuthorizationAttempt as requestOneDriveAuthorizationAttempt,
  beginOneDriveAuthorization as requestOneDriveAuthorizationBegin,
  cancelOneDriveAuthorization as requestOneDriveAuthorizationCancel,
  openStructuredView as requestOpenStructuredView,
  cancelOperation as requestOperationCancel,
  pauseOperation as requestOperationPause,
  resumeOperation as requestOperationResume,
  startOperation as requestOperationStart,
  listOperations as requestOperations,
  undoOperation as requestOperationUndo,
  setPaneActivity as requestPaneActivity,
  disablePlugin as requestPluginDisable,
  enablePlugin as requestPluginEnable,
  getPluginIconThemeAsset as requestPluginIconThemeAsset,
  getPluginLogs as requestPluginLogs,
  listPlugins as requestPlugins,
  closePptxPreview as requestPptxPreviewClose,
  openPptxPreview as requestPptxPreviewOpen,
  readPptxPreviewPdf as requestPptxPreviewPdf,
  readFileRange as requestReadFileRange,
  getRuntimeCapabilities as requestRuntimeCapabilities,
  saveEditableFile as requestSaveEditableFile,
  cancelSearch as requestSearchCancel,
  searchInFile as requestSearchInFile,
  startSearch as requestSearchStart,
  resumeSemanticCleanup as requestSemanticCleanupResume,
  getSemanticComponentCapabilities as requestSemanticComponentCapabilities,
  moveSemanticComponentData as requestSemanticComponentDataMove,
  pauseSemanticComponentIndexing as requestSemanticComponentIndexingPause,
  resumeSemanticComponentIndexing as requestSemanticComponentIndexingResume,
  confirmSemanticComponentIndexRemoval as requestSemanticComponentIndexRemovalConfirmation,
  createSemanticComponentIndexRemovalPlan as requestSemanticComponentIndexRemovalPlanCreation,
  getSemanticComponentStatus as requestSemanticComponentStatus,
  installSemanticComponentWorkerPatch as requestSemanticComponentWorkerPatch,
  updateSemanticEligibilityOverrides as requestSemanticEligibilityOverridesUpdate,
  confirmSemanticEnrolment as requestSemanticEnrolmentConfirmation,
  previewSemanticEnrolment as requestSemanticEnrolmentPreview,
  confirmSemanticExclusion as requestSemanticExclusionConfirmation,
  planSemanticExclusion as requestSemanticExclusionPlan,
  getSemanticFolderStatus as requestSemanticFolderStatus,
  acceptSemanticComponentInstallationOffer as requestSemanticInstallationOfferAcceptance,
  createSemanticComponentInstallationOffer as requestSemanticInstallationOfferCreation,
  getSemanticLibraryCapabilities as requestSemanticLibraryCapabilities,
  pauseSemanticLibrary as requestSemanticLibraryPause,
  resumeSemanticLibrary as requestSemanticLibraryResume,
  getSemanticLibraryStatus as requestSemanticLibraryStatus,
  importSemanticComponentLocalModel as requestSemanticLocalModelImport,
  checkpointSemanticComponentModelMigration as requestSemanticModelMigrationCheckpoint,
  completeSemanticComponentModelMigration as requestSemanticModelMigrationCompletion,
  confirmSemanticComponentModelMigration as requestSemanticModelMigrationConfirmation,
  planSemanticComponentModelMigration as requestSemanticModelMigrationPlan,
  listSemanticComponentProfiles as requestSemanticProfiles,
  uninstallSemanticComponents as requestSemanticUninstall,
  getSettings as requestSettings,
  updateSettings as requestSettingsUpdate,
  getSpotlightComment as requestSpotlightComment,
  setSpotlightComment as requestSpotlightCommentUpdate,
  acceptSshHostKey as requestSshHostKeyAcceptance,
  probeSshHostKey as requestSshHostKeyProbe,
  readStructuredJsonWindow as requestStructuredJsonWindow,
  searchStructuredRows as requestStructuredRowSearch,
  readStructuredRows as requestStructuredRows,
  closeStructuredView as requestStructuredViewClose,
  getStructuredViewStatus as requestStructuredViewStatus,
  updateStructuredView as requestStructuredViewUpdate,
  applySyncPlan as requestSyncPlanApply,
  generateSyncPlan as requestSyncPlanGenerate,
  getSystemLocations as requestSystemLocations,
  getThumbnail as requestThumbnail,
  getVolumes as requestVolumes,
  getWorkspace as requestWorkspace,
  applyWorkspaceCommand as requestWorkspaceCommand,
  createWorkspace as requestWorkspaceCreation,
  deleteWorkspace as requestWorkspaceDeletion,
  openWorkspace as requestWorkspaceOpen,
  startWorkspace as requestWorkspaceStart,
  listWorkspaces as requestWorkspaces,
} from '../generated/file-manager-api';
import type { ActionDescriptorDto } from '../generated/models/actionDescriptorDto';
import type { InvokeActionRequestDtoParameters } from '../generated/models/invokeActionRequestDtoParameters';
import type { PluginIconThemeDto } from '../generated/models/pluginIconThemeDto';
import { getSessionToken } from '../session-token';
import type { FileManagerClient, NativeFileDrop } from './file-manager-client';
import { trustedOneDriveAuthorizationUrl } from './onedrive-authorization-url';
import { operationFromDto } from './operation-mapping';
import { settingsFromDto, settingsToDto } from './settings-mapping';

/**
 * HTTP transport adapter, wrapping the Orval-generated client behind
 * `FileManagerClient` (spec §12). Only methods with a generated endpoint are
 * implemented; the rest throw {@link NotImplementedError} naming the backend
 * task that will add their endpoint (mirrors the stub from task 0011).
 * `RuntimeCapabilitiesDto`/`RuntimeCapabilities` are the same type (see
 * `models/runtime-capabilities.ts`), so no DTO mapping step is needed there;
 * once further endpoints land, keep any real DTO → model mapping in one
 * shared module so the Tauri/mock adapters can reuse it.
 */
export class HttpFileManagerClient implements FileManagerClient {
  private readonly eventStream = new SseEventStream({ tokenProvider: getSessionToken });
  readonly connection = this.eventStream.status;

  startNativeDrag(_locations: readonly FileLocation[], _signal?: AbortSignal): Promise<void> {
    return Promise.reject(new Error('Native drag is available only in the desktop application'));
  }

  showPlatformContextMenu(
    _locations: readonly FileLocation[],
    _signal?: AbortSignal,
  ): Promise<void> {
    return Promise.reject(
      new Error('Platform context menus are available only in the desktop application'),
    );
  }

  subscribeNativeFileDrops(_listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe> {
    return Promise.resolve(() => undefined);
  }

  async cacheArchivePassword(
    request: ArchiveCredentialRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await requestArchivePasswordCache(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204) {
      throw new Error(`Unexpected cacheArchivePassword response status: ${response.status}`);
    }
  }
  async getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities> {
    const response = await requestRuntimeCapabilities(
      signal !== undefined ? { signal } : undefined,
    );
    return response.data;
  }

  async getSemanticComponentCapabilities(
    signal?: AbortSignal,
  ): Promise<SemanticComponentCapabilities> {
    const response = await requestSemanticComponentCapabilities(
      signal === undefined ? undefined : { signal },
    );
    return response.data;
  }

  async getSemanticComponentStatus(signal?: AbortSignal): Promise<SemanticComponentStatus> {
    const response = await requestSemanticComponentStatus(
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getSemanticComponentStatus response status: ${response.status}`);
    }
    return response.data;
  }

  async listSemanticComponentProfiles(signal?: AbortSignal): Promise<SemanticModelProfile[]> {
    const response = await requestSemanticProfiles(signal === undefined ? undefined : { signal });
    if (response.status !== 200) {
      throw new Error(
        `Unexpected listSemanticComponentProfiles response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async createSemanticComponentInstallationOffer(
    request: CreateSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallationOffer> {
    const response = await requestSemanticInstallationOfferCreation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected createSemanticComponentInstallationOffer response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async acceptSemanticComponentInstallationOffer(
    request: AcceptSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallReceipt> {
    const response = await requestSemanticInstallationOfferAcceptance(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected acceptSemanticComponentInstallationOffer response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async pauseSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    const response = await requestSemanticComponentIndexingPause(
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204) {
      throw new Error(
        `Unexpected pauseSemanticComponentIndexing response status: ${response.status}`,
      );
    }
  }

  async resumeSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    const response = await requestSemanticComponentIndexingResume(
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204) {
      throw new Error(
        `Unexpected resumeSemanticComponentIndexing response status: ${response.status}`,
      );
    }
  }

  async createSemanticComponentIndexRemovalPlan(
    request: CreateSemanticIndexRemovalPlanRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalPlan> {
    const response = await requestSemanticComponentIndexRemovalPlanCreation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected createSemanticComponentIndexRemovalPlan response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async confirmSemanticComponentIndexRemoval(
    request: ConfirmSemanticIndexRemovalRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalReceipt> {
    const response = await requestSemanticComponentIndexRemovalConfirmation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected confirmSemanticComponentIndexRemoval response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async moveSemanticComponentData(
    request: MoveSemanticDataRequest,
    signal?: AbortSignal,
  ): Promise<SemanticDataMoveReceipt> {
    const response = await requestSemanticComponentDataMove(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected moveSemanticComponentData response status: ${response.status}`);
    }
    return response.data;
  }

  async uninstallSemanticComponents(
    request: UninstallSemanticComponentsRequest,
    signal?: AbortSignal,
  ): Promise<SemanticUninstallReceipt> {
    const response = await requestSemanticUninstall(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected uninstallSemanticComponents response status: ${response.status}`);
    }
    return response.data;
  }

  async installSemanticComponentWorkerPatch(
    request: InstallSemanticWorkerPatchRequest,
    signal?: AbortSignal,
  ): Promise<SemanticWorkerPatchResponse> {
    const response = await requestSemanticComponentWorkerPatch(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected installSemanticComponentWorkerPatch response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async importSemanticComponentLocalModel(
    request: ImportSemanticLocalModelRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    const response = await requestSemanticLocalModelImport(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected importSemanticComponentLocalModel response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async planSemanticComponentModelMigration(
    request: PlanSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    const response = await requestSemanticModelMigrationPlan(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected planSemanticComponentModelMigration response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async confirmSemanticComponentModelMigration(
    request: ConfirmSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    const response = await requestSemanticModelMigrationConfirmation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected confirmSemanticComponentModelMigration response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async checkpointSemanticComponentModelMigration(
    request: CheckpointSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    const response = await requestSemanticModelMigrationCheckpoint(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected checkpointSemanticComponentModelMigration response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async completeSemanticComponentModelMigration(
    request: CompleteSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelSelection> {
    const response = await requestSemanticModelMigrationCompletion(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected completeSemanticComponentModelMigration response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async getSemanticLibraryCapabilities(signal?: AbortSignal): Promise<SemanticLibraryCapabilities> {
    const response = await requestSemanticLibraryCapabilities(
      signal === undefined ? undefined : { signal },
    );
    return response.data;
  }

  async getSemanticLibraryStatus(signal?: AbortSignal): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticLibraryStatus(
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getSemanticLibraryStatus response status: ${response.status}`);
    }
    return response.data;
  }

  async getSemanticFolderStatus(
    request: GetSemanticFolderStatusRequest,
    signal?: AbortSignal,
  ): Promise<SemanticFolderStatus> {
    const response = await requestSemanticFolderStatus(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getSemanticFolderStatus response status: ${response.status}`);
    }
    return response.data;
  }

  async previewSemanticEnrolment(
    request: PreviewSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticEnrolmentPreview> {
    const response = await requestSemanticEnrolmentPreview(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected previewSemanticEnrolment response status: ${response.status}`);
    }
    return response.data;
  }

  async confirmSemanticEnrolment(
    request: ConfirmSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticEnrolmentConfirmation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected confirmSemanticEnrolment response status: ${response.status}`);
    }
    return response.data;
  }

  async planSemanticExclusion(
    request: PlanSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticExclusionPlan> {
    const response = await requestSemanticExclusionPlan(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected planSemanticExclusion response status: ${response.status}`);
    }
    return response.data;
  }

  async confirmSemanticExclusion(
    request: ConfirmSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticExclusionConfirmation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected confirmSemanticExclusion response status: ${response.status}`);
    }
    return response.data;
  }

  async resumeSemanticCleanup(
    request: ResumeSemanticCleanupRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticCleanupResume(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected resumeSemanticCleanup response status: ${response.status}`);
    }
    return response.data;
  }

  async pauseSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticLibraryPause(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected pauseSemanticLibrary response status: ${response.status}`);
    }
    return response.data;
  }

  async resumeSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticLibraryResume(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected resumeSemanticLibrary response status: ${response.status}`);
    }
    return response.data;
  }

  async updateSemanticEligibilityOverrides(
    request: UpdateSemanticEligibilityOverridesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    const response = await requestSemanticEligibilityOverridesUpdate(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected updateSemanticEligibilityOverrides response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async scanDiskUsage(request: ScanDiskUsageRequest, signal?: AbortSignal): Promise<void> {
    const response = await requestDiskUsageScan(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 202) {
      throw new Error(`Unexpected scanDiskUsage response status: ${response.status}`);
    }
  }

  async cancelDiskUsage(scanId: string, signal?: AbortSignal): Promise<void> {
    const response = await requestDiskUsageCancel(
      scanId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204) {
      throw new Error(`Unexpected cancelDiskUsage response status: ${response.status}`);
    }
  }

  async getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult> {
    const response = await requestDiagnostics(signal === undefined ? undefined : { signal });
    return response.data;
  }

  async getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]> {
    const response = await requestSystemLocations(signal === undefined ? undefined : { signal });
    if (response.status !== 200) {
      throw new Error(`Unexpected getSystemLocations response status: ${response.status}`);
    }
    return response.data.map((item) => ({
      name: item.name,
      kind: item.kind,
      location: { ...item.location },
      ...(item.providerHint == null ? {} : { providerHint: item.providerHint }),
      ...(item.protocol == null ? {} : { protocol: item.protocol }),
      ...(item.server == null ? {} : { server: item.server }),
      ...(item.share == null ? {} : { share: item.share }),
      ...(item.readOnly == null ? {} : { readOnly: item.readOnly }),
    }));
  }

  async getVolumes(signal?: AbortSignal): Promise<Volume[]> {
    const response = await requestVolumes(signal === undefined ? undefined : { signal });
    if (response.status !== 200) {
      throw new Error(`Unexpected getVolumes response status: ${response.status}`);
    }
    return response.data.map((item) => ({ name: item.name, location: { ...item.location } }));
  }

  // Home-directory expansion (`~` in an address bar) is a convenience for the desktop host; the
  // networked server has no notion of "the user's" home directory to report, so `~` simply isn't
  // expanded in this mode instead of guessing at a server-side path.
  async getHomeDirectory(_signal?: AbortSignal): Promise<string | undefined> {
    return undefined;
  }

  async getFileIcon(
    sampleLocationUri: string,
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    try {
      const response = await requestFileIcon(
        { uri: sampleLocationUri },
        signal === undefined ? undefined : { signal },
      );
      if (response.status !== 200) return undefined;
      return new Uint8Array(await response.data.arrayBuffer());
    } catch {
      return undefined;
    }
  }

  async getThumbnail(
    locationUri: string,
    size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    try {
      const response = await requestThumbnail(
        { uri: locationUri, size },
        signal === undefined ? undefined : { signal },
      );
      if (response.status !== 200) return undefined;
      return new Uint8Array(await response.data.arrayBuffer());
    } catch {
      return undefined;
    }
  }

  async getFinderTags(locationUri: string, signal?: AbortSignal): Promise<FinderTags | undefined> {
    try {
      const response = await requestFinderTags(
        { uri: locationUri },
        signal === undefined ? undefined : { signal },
      );
      if (response.status !== 200) return undefined;
      return response.data;
    } catch {
      return undefined;
    }
  }

  async setFinderTags(
    locationUri: string,
    tags: FinderTags,
    signal?: AbortSignal,
  ): Promise<FinderTags> {
    const response = await requestFinderTagsUpdate(
      tags,
      { uri: locationUri },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected setFinderTags response status: ${response.status}`);
    }
    return response.data;
  }

  async getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined> {
    try {
      const response = await requestSpotlightComment(
        { uri: locationUri },
        signal === undefined ? undefined : { signal },
      );
      if (response.status !== 200) return undefined;
      return response.data;
    } catch {
      return undefined;
    }
  }

  async setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment> {
    const response = await requestSpotlightCommentUpdate(
      comment,
      { uri: locationUri },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected setSpotlightComment response status: ${response.status}`);
    }
    return response.data;
  }

  async getSettings(signal?: AbortSignal): Promise<Settings> {
    return settingsFromDto(
      (await requestSettings(signal === undefined ? undefined : { signal })).data,
    );
  }

  async updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings> {
    const response = await requestSettingsUpdate(
      settingsToDto(settings),
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected updateSettings response status: ${response.status}`);
    }
    return settingsFromDto(response.data);
  }

  async listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]> {
    const response = await requestWorkspaces(signal === undefined ? undefined : { signal });
    return response.data;
  }

  async startWorkspace(
    workspaceId?: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    const response = await requestWorkspaceStart(
      workspaceId === undefined ? undefined : { workspaceId },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected startWorkspace response status: ${response.status}`);
    }
    return workspaceProjectionFromDto(response.data, { redirectSessionOnlyTabs: true });
  }

  async createWorkspace(
    request: CreateWorkspaceRequest,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    const response = await requestWorkspaceCreation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected createWorkspace response status: ${response.status}`);
    }
    return workspaceProjectionFromDto(response.data);
  }

  async getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    const response = await requestWorkspace(
      workspaceId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getWorkspace response status: ${response.status}`);
    }
    return workspaceProjectionFromDto(response.data, { redirectSessionOnlyTabs: true });
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

  async deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void> {
    await requestWorkspaceDeletion(
      workspaceId,
      expectedRevision === undefined ? undefined : { expectedRevision },
      signal === undefined ? undefined : { signal },
    );
  }

  async openWorkspace(
    workspaceId: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    const response = await requestWorkspaceOpen(
      workspaceId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected openWorkspace response status: ${response.status}`);
    }
    return workspaceProjectionFromDto(response.data, { redirectSessionOnlyTabs: true });
  }

  async dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    const response = await requestWorkspaceCommand(
      command.workspaceId,
      command,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected applyWorkspaceCommand response status: ${response.status}`);
    }
    return workspaceProjectionFromDto(response.data);
  }

  async navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    const response = await requestNavigation(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected navigatePane response status: ${response.status}`);
    }
    return directorySnapshotFromDto(response.data);
  }

  async listDirectory(
    request: ListDirectoryRequest,
    signal?: AbortSignal,
  ): Promise<DirectorySnapshot> {
    const response = await requestDirectory(request, signal !== undefined ? { signal } : undefined);
    if (response.status !== 200) {
      // Surface the backend's actual `ApplicationErrorDto.message` (e.g. "unsupported RAR
      // compression method") rather than a generic status-code message - this is the archive
      // listing path the comic (.cbz/.cbr) and EPUB viewers depend on, where the underlying
      // cause (an archive format/feature the backend's reader can't handle) is exactly what a
      // user needs to see to tell "this app has a bug" from "this file can't be read".
      const message =
        typeof response.data === 'object' && response.data !== null && 'message' in response.data
          ? String((response.data as { message: unknown }).message)
          : `Unexpected listDirectory response status: ${response.status}`;
      throw new Error(message);
    }
    return directorySnapshotFromDto(response.data);
  }

  async listDirectoryChildren(
    location: FileLocation,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]> {
    const response = await requestDirectoryChildren(
      { location, showHidden },
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected listDirectoryChildren response status: ${response.status}`);
    }
    return response.data.map(entrySummaryFromDto);
  }

  async getEntryMetadata(
    request: EntryMetadataRequest,
    signal?: AbortSignal,
  ): Promise<EntryMetadata> {
    const response = await requestEntryMetadata(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getEntryMetadata response status: ${response.status}`);
    }
    return entryMetadataFromDto(response.data);
  }

  async setPaneActivity(request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void> {
    const response = await requestPaneActivity(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 204) {
      throw new Error(`Unexpected setPaneActivity response status: ${response.status}`);
    }
  }

  async readFileRange(
    request: ReadFileRangeRequest,
    signal?: AbortSignal,
  ): Promise<FileRangeChunk> {
    const response = await requestReadFileRange(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected readFileRange response status: ${response.status}`);
    }
    return response.data;
  }

  async openDocxPreview(
    request: OpenDocxPreviewRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreview> {
    const response = await requestDocxPreviewOpen(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected openDocxPreview response status: ${response.status}`);
    return response.data;
  }

  async readDocxPreviewResource(
    request: ReadDocxPreviewResourceRequest,
    signal?: AbortSignal,
  ): Promise<DocxPreviewResource> {
    const response = await requestDocxPreviewResource(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected readDocxPreviewResource response status: ${response.status}`);
    return response.data;
  }

  async closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    const response = await requestDocxPreviewClose(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 204)
      throw new Error(`Unexpected closeDocxPreview response status: ${response.status}`);
  }

  async openPptxPreview(
    request: OpenPptxPreviewRequest,
    signal?: AbortSignal,
  ): Promise<PptxPreview> {
    const response = await requestPptxPreviewOpen(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected openPptxPreview response status: ${response.status}`);
    return response.data;
  }

  async readPptxPreviewPdf(
    request: ReadPptxPreviewPdfRequest,
    signal?: AbortSignal,
  ): Promise<FileRangeChunk> {
    const response = await requestPptxPreviewPdf(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected readPptxPreviewPdf response status: ${response.status}`);
    return response.data;
  }

  async closePptxPreview(request: PptxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    const response = await requestPptxPreviewClose(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 204)
      throw new Error(`Unexpected closePptxPreview response status: ${response.status}`);
  }

  async openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    const response = await requestOpenStructuredView(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected openStructuredView response status: ${response.status}`);
    return response.data;
  }

  async getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus> {
    const response = await requestStructuredViewStatus(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected getStructuredViewStatus response status: ${response.status}`);
    return response.data;
  }

  async updateStructuredView(
    request: UpdateStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    const response = await requestStructuredViewUpdate(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected updateStructuredView response status: ${response.status}`);
    return response.data;
  }

  async readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows> {
    const response = await requestStructuredRows(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected readStructuredRows response status: ${response.status}`);
    return response.data;
  }

  async readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    signal?: AbortSignal,
  ): Promise<StructuredJsonWindow> {
    const response = await requestStructuredJsonWindow(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected readStructuredJsonWindow response status: ${response.status}`);
    return response.data;
  }

  async searchStructuredRows(
    request: SearchStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRowSearch> {
    const response = await requestStructuredRowSearch(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected searchStructuredRows response status: ${response.status}`);
    return response.data;
  }

  async closeStructuredView(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await requestStructuredViewClose(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 204)
      throw new Error(`Unexpected closeStructuredView response status: ${response.status}`);
  }

  async loadEditableFile(
    request: LoadEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFile> {
    const response = await requestLoadEditableFile(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected loadEditableFile response status: ${response.status}`);
    return response.data;
  }

  async saveEditableFile(
    request: SaveEditableFileRequest,
    signal?: AbortSignal,
  ): Promise<EditableFileSave> {
    const response = await requestSaveEditableFile(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200)
      throw new Error(`Unexpected saveEditableFile response status: ${response.status}`);
    return response.data;
  }

  async searchInFile(
    request: SearchInFileRequest,
    signal?: AbortSignal,
  ): Promise<SearchInFileResult> {
    const response = await requestSearchInFile(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected searchInFile response status: ${response.status}`);
    }
    return response.data;
  }

  async calculateFolderSize(
    request: CalculateFolderSizeRequest,
    signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult> {
    const response = await requestFolderSizeCalculation(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected calculateFolderSize response status: ${response.status}`);
    }
    return response.data;
  }

  async archiveSummary(
    request: ArchiveSummaryRequest,
    signal?: AbortSignal,
  ): Promise<ArchiveSummaryResult> {
    const response = await requestArchiveSummary(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected archiveSummary response status: ${response.status}`);
    }
    return response.data;
  }

  async discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult> {
    const response = await requestApplicationUninstallDiscovery(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected discoverApplicationUninstallCandidates response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult> {
    const response = await requestApplicationDockIconRemoval(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected removeApplicationDockIcon response status: ${response.status}`);
    }
    return response.data;
  }

  async gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult> {
    const response = await requestGitFileHistory(
      request,
      signal !== undefined ? { signal } : undefined,
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected gitFileHistory response status: ${response.status}`);
    }
    return response.data;
  }

  async startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation> {
    const { destinations, archiveCompressionLevel, ...rest } = request;
    const response = await requestOperationStart(
      {
        ...rest,
        sources: [...rest.sources],
        ...(destinations === undefined ? {} : { destinations: [...destinations] }),
        ...(archiveCompressionLevel === undefined ? {} : { archiveCompressionLevel }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected startOperation response status: ${response.status}`);
    }
    return operationFromDto(response.data);
  }

  async listOperations(signal?: AbortSignal): Promise<Operation[]> {
    const response = await requestOperations(
      undefined,
      signal === undefined ? undefined : { signal },
    );
    return response.data.operations.map(operationFromDto);
  }

  async cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    const response = await requestOperationCancel(
      operationId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected cancelOperation response status: ${response.status}`);
  }

  async undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation> {
    const response = await requestOperationUndo(
      operationId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201)
      throw new Error(`Unexpected undoOperation response status: ${response.status}`);
    return operationFromDto(response.data);
  }

  async pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    const response = await requestOperationPause(
      operationId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected pauseOperation response status: ${response.status}`);
  }

  async resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    const response = await requestOperationResume(
      operationId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected resumeOperation response status: ${response.status}`);
  }

  async resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void> {
    const response = await requestConflictResolution(
      request.operationId,
      { resolution: request.resolution, applyToAllSimilar: request.applyToAllSimilar },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected resolveConflict response status: ${response.status}`);
  }

  async startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult> {
    const response = await requestSearchStart(
      {
        query: request.query,
        ...(request.contentQuery === undefined ? {} : { contentQuery: request.contentQuery }),
        ...(request.contentRegex === undefined ? {} : { contentRegex: request.contentRegex }),
        ...(request.contentCaseSensitive === undefined
          ? {}
          : { contentCaseSensitive: request.contentCaseSensitive }),
        ...(request.contentWholeWord === undefined
          ? {}
          : { contentWholeWord: request.contentWholeWord }),
        ...(request.recurse === undefined ? {} : { recurse: request.recurse }),
        ...(request.showHidden === undefined ? {} : { showHidden: request.showHidden }),
        roots: [...request.roots],
        ...(request.structuredQuery === undefined
          ? {}
          : {
              structuredQuery: {
                ...request.structuredQuery,
                scope: {
                  ...request.structuredQuery.scope,
                  locations: [...request.structuredQuery.scope.locations],
                },
                entryKinds: [...request.structuredQuery.entryKinds],
                mimeTypes: [...request.structuredQuery.mimeTypes],
                gitStatuses: [...request.structuredQuery.gitStatuses],
                tags: [...request.structuredQuery.tags],
                metadata: { ...request.structuredQuery.metadata },
              },
            }),
        workspaceId: request.workspaceId,
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected startSearch response status: ${response.status}`);
    }
    return {
      searchId: response.data.searchId,
      location: response.data.location,
      limitations: response.data.limitations,
      executionMode: response.data.executionMode,
    };
  }

  async cancelSearch(searchId: string, signal?: AbortSignal): Promise<void> {
    const response = await requestSearchCancel(
      searchId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected cancelSearch response status: ${response.status}`);
  }

  async startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult> {
    const response = await requestComparisonStart(
      {
        workspaceId: request.workspaceId,
        left: request.left,
        right: request.right,
        criteria: request.criteria,
        ...(request.showHidden === undefined ? {} : { showHidden: request.showHidden }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected startComparison response status: ${response.status}`);
    }
    return { comparisonId: response.data.comparisonId };
  }

  async getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage> {
    const response = await requestComparisonGet(
      comparisonId,
      {
        ...(options?.offset === undefined ? {} : { offset: options.offset }),
        ...(options?.limit === undefined ? {} : { limit: options.limit }),
        ...(options?.differencesOnly === undefined
          ? {}
          : { differencesOnly: options.differencesOnly }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getComparison response status: ${response.status}`);
    }
    return comparisonPageFromDto(response.data);
  }

  async cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void> {
    const response = await requestComparisonCancel(
      comparisonId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected cancelComparison response status: ${response.status}`);
  }

  async startChecksums(
    request: StartChecksumRequest,
    signal?: AbortSignal,
  ): Promise<StartChecksumResult> {
    const response = await requestChecksumStart(
      {
        workspaceId: request.workspaceId,
        entries: request.entries,
        algorithms: request.algorithms,
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected startChecksums response status: ${response.status}`);
    }
    return { jobId: response.data.jobId };
  }

  async getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage> {
    const response = await requestChecksumGet(
      jobId,
      {
        ...(options?.offset === undefined ? {} : { offset: options.offset }),
        ...(options?.limit === undefined ? {} : { limit: options.limit }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getChecksums response status: ${response.status}`);
    }
    return checksumPageFromDto(response.data);
  }

  async cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void> {
    const response = await requestChecksumCancel(
      jobId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected cancelChecksums response status: ${response.status}`);
  }

  async renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile> {
    const response = await requestChecksumFileRender(
      jobId,
      { algorithm },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected renderChecksumFile response status: ${response.status}`);
    }
    return { suggestedName: response.data.suggestedName, content: response.data.content };
  }

  async saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile> {
    const response = await requestChecksumFileSave(
      jobId,
      {
        destination: request.destination,
        algorithm: request.algorithm,
        ...(request.overwrite === undefined ? {} : { overwrite: request.overwrite }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected saveChecksumFile response status: ${response.status}`);
    }
    return { location: response.data.location, bytesWritten: response.data.bytesWritten };
  }

  async verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport> {
    const response = await requestChecksumFileVerify(
      jobId,
      { content },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected verifyChecksumFile response status: ${response.status}`);
    }
    return verificationReportFromDto(response.data);
  }

  async startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult> {
    const response = await requestDuplicateScanStart(
      {
        workspaceId: request.workspaceId,
        roots: request.roots,
        ...(request.showHidden === undefined ? {} : { showHidden: request.showHidden }),
        ...(request.includeEmptyFiles === undefined
          ? {}
          : { includeEmptyFiles: request.includeEmptyFiles }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected startDuplicateScan response status: ${response.status}`);
    }
    return { scanId: response.data.scanId };
  }

  async getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage> {
    const response = await requestDuplicateScanGet(
      scanId,
      {
        ...(options?.offset === undefined ? {} : { offset: options.offset }),
        ...(options?.limit === undefined ? {} : { limit: options.limit }),
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected getDuplicateScan response status: ${response.status}`);
    }
    return duplicatePageFromDto(response.data);
  }

  async cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    const response = await requestDuplicateScanCancel(
      scanId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected cancelDuplicateScan response status: ${response.status}`);
  }

  async generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan> {
    const response = await requestSyncPlanGenerate(
      comparisonId,
      { mode: request.mode },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected generateSyncPlan response status: ${response.status}`);
    }
    return syncPlanFromDto(response.data);
  }

  async applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult> {
    const response = await requestSyncPlanApply(
      comparisonId,
      { items: request.items.map(syncPlanItemToDto) },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201) {
      throw new Error(`Unexpected applySyncPlan response status: ${response.status}`);
    }
    return { operationIds: response.data.operationIds as OperationId[] };
  }

  listActions(signal?: AbortSignal): Promise<ActionDescriptor[]> {
    return requestActions(signal === undefined ? undefined : { signal }).then((response) =>
      response.data.map(actionFromDto),
    );
  }

  async invokeAction(request: InvokeActionRequest, signal?: AbortSignal): Promise<ActionResult> {
    const response = await requestActionInvocation(
      request.actionId,
      {
        parameters: (request.parameters ?? null) as InvokeActionRequestDtoParameters,
        context: request.context,
      },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected invokeAction response status: ${response.status}`);
    }
    return {
      actionId: response.data.actionId,
      invoked: response.data.invoked,
      ...(response.data.operationId == null ? {} : { operationId: response.data.operationId }),
    };
  }

  async listPlugins(signal?: AbortSignal): Promise<PluginDescriptor[]> {
    const response = await requestPlugins(signal === undefined ? undefined : { signal });
    if (response.status !== 200)
      throw new Error(`Unexpected listPlugins response status: ${response.status}`);
    return response.data.map((plugin) => ({
      id: plugin.id,
      name: plugin.name,
      version: plugin.version,
      description: plugin.description,
      enabled: plugin.enabled,
      ...(plugin.diagnostic == null ? {} : { diagnostic: plugin.diagnostic }),
      ...(plugin.columns === undefined ? {} : { columns: plugin.columns }),
      ...(plugin.iconTheme == null ? {} : { iconTheme: pluginIconThemeFromDto(plugin.iconTheme) }),
      permissions: {
        selectedEntryMetadata: plugin.permissions.selectedEntryMetadata,
        selectedEntryContentRead: plugin.permissions.selectedEntryContentRead,
        filesystemRead: plugin.permissions.filesystemRead,
        filesystemWrite: plugin.permissions.filesystemWrite,
        clipboardRead: plugin.permissions.clipboardRead,
        clipboardWrite: plugin.permissions.clipboardWrite,
        network: plugin.permissions.network,
        processSpawn: plugin.permissions.processSpawn,
        notifications: plugin.permissions.notifications,
        settingsStorage: plugin.permissions.settingsStorage,
      },
    }));
  }

  async setPluginEnabled(
    pluginId: PluginId,
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<void> {
    const options = signal === undefined ? undefined : { signal };
    const response = enabled
      ? await requestPluginEnable(pluginId, options)
      : await requestPluginDisable(pluginId, options);
    if (response.status !== 204)
      throw new Error(`Unexpected setPluginEnabled response status: ${response.status}`);
  }

  async getPluginLogs(pluginId: PluginId, signal?: AbortSignal): Promise<PluginLogEntry[]> {
    const response = await requestPluginLogs(
      pluginId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected getPluginLogs response status: ${response.status}`);
    return response.data.map((entry) => ({ message: entry.message }));
  }

  async getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    signal?: AbortSignal,
  ): Promise<string> {
    const response = await requestPluginIconThemeAsset(
      pluginId,
      { path: assetPath },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected getPluginIconThemeAsset response status: ${response.status}`);
    return response.data;
  }

  async subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    const unsubscribe = this.eventStream.listeners.subscribe(listener);
    await this.eventStream.connect();
    return unsubscribe;
  }

  onResynchronise(listener: () => void): Unsubscribe {
    return this.eventStream.resynchronise.subscribe(listener);
  }

  disconnect(): void {
    this.eventStream.close();
  }

  async listConnections(signal?: AbortSignal): Promise<Connection[]> {
    const response = await requestConnections(signal === undefined ? undefined : { signal });
    if (response.status !== 200)
      throw new Error(`Unexpected listConnections response status: ${response.status}`);
    return response.data;
  }

  async createConnection(
    request: CreateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection> {
    const response = await requestConnectionCreation(
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 201)
      throw new Error(`Unexpected createConnection response status: ${response.status}`);
    return response.data;
  }

  async getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    const response = await requestConnection(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected getConnection response status: ${response.status}`);
    return response.data;
  }

  async updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection> {
    const response = await requestConnectionUpdate(
      connectionId,
      request,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected updateConnection response status: ${response.status}`);
    return response.data;
  }

  async deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void> {
    const response = await requestConnectionDeletion(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected deleteConnection response status: ${response.status}`);
  }

  async connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    const response = await requestConnectionConnect(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected connectConnection response status: ${response.status}`);
    return response.data;
  }

  async disconnectConnection(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<Connection> {
    const response = await requestConnectionDisconnect(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected disconnectConnection response status: ${response.status}`);
    return response.data;
  }

  async testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    const response = await requestConnectionTest(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected testConnection response status: ${response.status}`);
    return response.data;
  }

  async beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse> {
    const authorizationWindow = globalThis.open('', '_blank');
    if (authorizationWindow === null) {
      throw new Error('The browser blocked the Microsoft sign-in window.');
    }
    authorizationWindow.opener = null;
    try {
      const response = await requestOneDriveAuthorizationBegin(
        connectionId,
        signal === undefined ? undefined : { signal },
      );
      if (response.status !== 201) {
        throw new Error(
          `Unexpected beginOneDriveAuthorization response status: ${response.status}`,
        );
      }
      authorizationWindow.location.href = trustedOneDriveAuthorizationUrl(
        response.data.authorizationUrl,
      );
      return response.data;
    } catch (error) {
      authorizationWindow.close();
      throw error;
    }
  }

  async getOneDriveAuthorizationAttempt(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    const response = await requestOneDriveAuthorizationAttempt(
      attemptId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(
        `Unexpected getOneDriveAuthorizationAttempt response status: ${response.status}`,
      );
    }
    return response.data;
  }

  async cancelOneDriveAuthorization(
    attemptId: string,
    signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    const response = await requestOneDriveAuthorizationCancel(
      attemptId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200) {
      throw new Error(`Unexpected cancelOneDriveAuthorization response status: ${response.status}`);
    }
    return response.data;
  }

  async probeSshHostKey(connectionId: ConnectionId, signal?: AbortSignal): Promise<HostKeyProbe> {
    const response = await requestSshHostKeyProbe(
      connectionId,
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 200)
      throw new Error(`Unexpected probeSshHostKey response status: ${response.status}`);
    return response.data;
  }

  async acceptSshHostKey(
    connectionId: ConnectionId,
    fingerprint: string,
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await requestSshHostKeyAcceptance(
      connectionId,
      { fingerprint },
      signal === undefined ? undefined : { signal },
    );
    if (response.status !== 204)
      throw new Error(`Unexpected acceptSshHostKey response status: ${response.status}`);
  }
}

function pluginIconThemeFromDto(dto: PluginIconThemeDto): PluginIconTheme {
  return {
    iconDefinitions: dto.iconDefinitions,
    ...(dto.file == null ? {} : { file: dto.file }),
    ...(dto.folder == null ? {} : { folder: dto.folder }),
    ...(dto.symlink == null ? {} : { symlink: dto.symlink }),
    fileExtensions: dto.fileExtensions,
    fileNames: dto.fileNames,
    folderNames: dto.folderNames,
    folderNamesExpanded: dto.folderNamesExpanded,
    mimePrefixes: dto.mimePrefixes,
  };
}

function actionFromDto(dto: ActionDescriptorDto): ActionDescriptor {
  return {
    id: dto.id,
    title: dto.title,
    ...(dto.description == null ? {} : { description: dto.description }),
    category: dto.category,
    defaultShortcuts: dto.defaultShortcuts ?? [],
    contextRequirements: { ...dto.contextRequirements },
    ...(dto.parameterSchema == null ? {} : { parameterSchema: dto.parameterSchema }),
    source:
      dto.source.kind === 'plugin'
        ? { kind: 'plugin', pluginId: dto.source.pluginId }
        : { kind: 'core' },
  };
}
