import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { openUrl } from '@tauri-apps/plugin-opener';

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
import { syncPlanItemToDto } from '../../models/comparison';
import { entryMetadataFromDto, entrySummaryFromDto } from '../../models/entry';
import { directorySnapshotFromDto } from '../../models/snapshot';
import { workspaceProjectionFromDto } from '../../models/workspace';
import { TauriEventStream } from '../events/tauri-event-stream';
import type { DirectorySnapshotDto } from '../generated/models/directorySnapshotDto';
import type { EntryMetadataDto } from '../generated/models/entryMetadataDto';
import type { EntrySummaryDto } from '../generated/models/entrySummaryDto';
import type { SettingsDto } from '../generated/models/settingsDto';
import type { WorkspaceDto } from '../generated/models/workspaceDto';
import type { FileManagerClient, NativeFileDrop } from './file-manager-client';
import { trustedOneDriveAuthorizationUrl } from './onedrive-authorization-url';
import { settingsFromDto, settingsToDto } from './settings-mapping';

/**
 * Tauri transport adapter, calling `FileManagerService` through `invoke`
 * (spec §11, §12). Only commands registered on the Rust side (task 0015) are
 * implemented; the rest throw {@link NotImplementedError} naming the task
 * that will add their command, mirroring `HttpFileManagerClient`.
 */
export class TauriFileManagerClient implements FileManagerClient {
  private readonly eventStream = new TauriEventStream();
  readonly connection = this.eventStream.status;

  cacheArchivePassword(request: ArchiveCredentialRequest, _signal?: AbortSignal): Promise<void> {
    return invoke<void>('cache_archive_password', { request });
  }

  async getRuntimeCapabilities(_signal?: AbortSignal): Promise<RuntimeCapabilities> {
    return invoke<RuntimeCapabilities>('get_runtime_capabilities');
  }

  async getDiagnostics(_signal?: AbortSignal): Promise<DiagnosticsResult> {
    return invoke<DiagnosticsResult>('get_diagnostics');
  }

  async getSystemLocations(_signal?: AbortSignal): Promise<SystemLocation[]> {
    return invoke<SystemLocation[]>('get_system_locations');
  }

  async getVolumes(_signal?: AbortSignal): Promise<Volume[]> {
    return invoke<Volume[]>('get_volumes');
  }

  async getHomeDirectory(_signal?: AbortSignal): Promise<string | undefined> {
    return (await invoke<string | null>('get_home_directory')) ?? undefined;
  }

  startNativeDrag(locations: readonly Location[], _signal?: AbortSignal): Promise<void> {
    return invoke<void>('start_native_drag', { locations });
  }

  showPlatformContextMenu(locations: readonly Location[], _signal?: AbortSignal): Promise<void> {
    return invoke<void>('show_platform_context_menu', { locations });
  }

  async quit(): Promise<void> {
    await getCurrentWindow().close();
  }

  async openWorkspaceWindow(sourceWorkspaceId?: WorkspaceId): Promise<void> {
    await invoke<void>('open_workspace_window', { sourceWorkspaceId });
  }

  async resyncWorkspace(
    ephemeralWorkspaceId: WorkspaceId,
    targetWorkspaceId?: WorkspaceId,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(
      await invoke<WorkspaceDto>('resync_workspace', {
        workspaceId: ephemeralWorkspaceId,
        targetWorkspaceId,
      }),
    );
  }

  subscribeNativeFileDrops(listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe> {
    return getCurrentWindow().onDragDropEvent(async ({ payload }) => {
      if (payload.type !== 'drop') return;
      const locations = await invoke<Location[]>('native_drag_locations', {
        paths: payload.paths,
      });
      listener({ locations, position: payload.position });
    });
  }

  async getFileIcon(
    sampleLocationUri: string,
    _signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    try {
      return new Uint8Array(await invoke<number[]>('get_file_icon', { uri: sampleLocationUri }));
    } catch {
      return undefined;
    }
  }

  async getThumbnail(
    locationUri: string,
    size: 'small' | 'medium' | 'large',
    _signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    try {
      return new Uint8Array(await invoke<number[]>('get_thumbnail', { uri: locationUri, size }));
    } catch {
      return undefined;
    }
  }

  async getFinderTags(locationUri: string, _signal?: AbortSignal): Promise<FinderTags | undefined> {
    try {
      return await invoke<FinderTags>('get_finder_tags', { uri: locationUri });
    } catch {
      return undefined;
    }
  }

  setFinderTags(locationUri: string, tags: FinderTags, _signal?: AbortSignal): Promise<FinderTags> {
    return invoke<FinderTags>('set_finder_tags', { uri: locationUri, request: tags });
  }

  async getSpotlightComment(
    locationUri: string,
    _signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined> {
    try {
      return await invoke<SpotlightComment>('get_spotlight_comment', { uri: locationUri });
    } catch {
      return undefined;
    }
  }

  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    _signal?: AbortSignal,
  ): Promise<SpotlightComment> {
    return invoke<SpotlightComment>('set_spotlight_comment', {
      uri: locationUri,
      request: comment,
    });
  }

  async getSettings(_signal?: AbortSignal): Promise<Settings> {
    return settingsFromDto(await invoke<SettingsDto>('get_settings'));
  }

  async updateSettings(settings: Settings, _signal?: AbortSignal): Promise<Settings> {
    return settingsFromDto(
      await invoke<SettingsDto>('update_settings', { settings: settingsToDto(settings) }),
    );
  }

  listWorkspaces(_signal?: AbortSignal): Promise<WorkspaceSummary[]> {
    return invoke<WorkspaceSummary[]>('list_workspaces');
  }

  async startWorkspace(
    workspaceId?: WorkspaceId,
    _signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(
      await invoke<WorkspaceDto>('start_workspace', { workspaceId }),
      { redirectSessionOnlyTabs: true },
    );
  }

  async createWorkspace(
    request: CreateWorkspaceRequest,
    _signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(await invoke<WorkspaceDto>('create_workspace', { request }));
  }

  async getWorkspace(
    workspaceId: WorkspaceId,
    _signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(
      await invoke<WorkspaceDto>('get_workspace', { workspaceId }),
      {
        redirectSessionOnlyTabs: true,
      },
    );
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
    _signal?: AbortSignal,
  ): Promise<void> {
    await invoke('delete_workspace', { workspaceId, expectedRevision });
  }

  async openWorkspace(
    workspaceId: WorkspaceId,
    _signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(
      await invoke<WorkspaceDto>('open_workspace', { workspaceId }),
      {
        redirectSessionOnlyTabs: true,
      },
    );
  }

  async dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    _signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return workspaceProjectionFromDto(
      await invoke<WorkspaceDto>('apply_workspace_command', { command }),
    );
  }

  async navigatePane(request: NavigateRequest, _signal?: AbortSignal): Promise<DirectorySnapshot> {
    return directorySnapshotFromDto(
      await invoke<DirectorySnapshotDto>('navigate_pane', { request }),
    );
  }

  async listDirectory(
    request: ListDirectoryRequest,
    _signal?: AbortSignal,
  ): Promise<DirectorySnapshot> {
    return directorySnapshotFromDto(
      await invoke<DirectorySnapshotDto>('list_directory', { request }),
    );
  }

  async listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    _signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]> {
    const children = await invoke<EntrySummaryDto[]>('list_directory_children', {
      request: { location, showHidden },
    });
    return children.map(entrySummaryFromDto);
  }

  async getEntryMetadata(
    request: EntryMetadataRequest,
    _signal?: AbortSignal,
  ): Promise<EntryMetadata> {
    return entryMetadataFromDto(await invoke<EntryMetadataDto>('get_entry_metadata', { request }));
  }

  async setPaneActivity(request: SetPaneActivityRequest, _signal?: AbortSignal): Promise<void> {
    await invoke<void>('set_pane_activity', { request });
  }

  readFileRange(request: ReadFileRangeRequest, _signal?: AbortSignal): Promise<FileRangeChunk> {
    return invoke<FileRangeChunk>('read_file_range', { request });
  }

  openStructuredView(
    request: OpenStructuredViewRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredView> {
    return invoke<StructuredView>('open_structured_view', { request });
  }

  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredViewStatus> {
    return invoke<StructuredViewStatus>('structured_view_status', { request });
  }

  updateStructuredView(
    request: UpdateStructuredViewRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredView> {
    return invoke<StructuredView>('update_structured_view', { request });
  }

  readStructuredRows(
    request: ReadStructuredRowsRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredRows> {
    return invoke<StructuredRows>('read_structured_rows', { request });
  }

  readStructuredJsonWindow(
    request: ReadStructuredJsonWindowRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredJsonWindow> {
    return invoke<StructuredJsonWindow>('read_structured_json_window', { request });
  }

  searchStructuredRows(
    request: SearchStructuredRowsRequest,
    _signal?: AbortSignal,
  ): Promise<StructuredRowSearch> {
    return invoke<StructuredRowSearch>('search_structured_rows', { request });
  }

  async closeStructuredView(
    request: StructuredViewSessionRequest,
    _signal?: AbortSignal,
  ): Promise<void> {
    await invoke<void>('close_structured_view', { request });
  }

  loadEditableFile(request: LoadEditableFileRequest, _signal?: AbortSignal): Promise<EditableFile> {
    return invoke<EditableFile>('load_editable_file', { request });
  }

  saveEditableFile(
    request: SaveEditableFileRequest,
    _signal?: AbortSignal,
  ): Promise<EditableFileSave> {
    return invoke<EditableFileSave>('save_editable_file', { request });
  }

  searchInFile(request: SearchInFileRequest, _signal?: AbortSignal): Promise<SearchInFileResult> {
    return invoke<SearchInFileResult>('search_in_file', { request });
  }

  calculateFolderSize(
    request: CalculateFolderSizeRequest,
    _signal?: AbortSignal,
  ): Promise<CalculateFolderSizeResult> {
    return invoke<CalculateFolderSizeResult>('calculate_folder_size', { request });
  }

  scanDiskUsage(request: ScanDiskUsageRequest, _signal?: AbortSignal): Promise<void> {
    return invoke<void>('scan_disk_usage', { request });
  }

  async cancelDiskUsage(scanId: string, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_disk_usage', { scanId });
  }

  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    _signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult> {
    return invoke<DiscoverApplicationUninstallCandidatesResult>(
      'discover_application_uninstall_candidates',
      { request },
    );
  }

  removeApplicationDockIcon(
    request: RemoveApplicationDockIconRequest,
    _signal?: AbortSignal,
  ): Promise<RemoveApplicationDockIconResult> {
    return invoke<RemoveApplicationDockIconResult>('remove_application_dock_icon', { request });
  }

  gitFileHistory(
    request: GitFileHistoryRequest,
    _signal?: AbortSignal,
  ): Promise<GitFileHistoryResult> {
    return invoke<GitFileHistoryResult>('get_file_git_history', { request });
  }

  startOperation(request: StartOperationRequest, _signal?: AbortSignal): Promise<Operation> {
    return invoke<Operation>('start_operation', { request });
  }

  listOperations(_signal?: AbortSignal): Promise<Operation[]> {
    return invoke<Operation[]>('list_operations');
  }

  async cancelOperation(operationId: OperationId, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_operation', { operationId });
  }

  async pauseOperation(operationId: OperationId, _signal?: AbortSignal): Promise<void> {
    await invoke('pause_operation', { operationId });
  }

  async resumeOperation(operationId: OperationId, _signal?: AbortSignal): Promise<void> {
    await invoke('resume_operation', { operationId });
  }

  async resolveConflict(request: ResolveConflictRequest, _signal?: AbortSignal): Promise<void> {
    await invoke('resolve_operation_conflict', {
      operationId: request.operationId,
      request: {
        resolution: request.resolution,
        applyToAllSimilar: request.applyToAllSimilar,
      },
    });
  }

  startSearch(request: StartSearchRequest, _signal?: AbortSignal): Promise<StartSearchResult> {
    return invoke<StartSearchResult>('start_search', { request });
  }

  async cancelSearch(searchId: string, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_search', { searchId });
  }

  startComparison(
    request: StartComparisonRequest,
    _signal?: AbortSignal,
  ): Promise<StartComparisonResult> {
    return invoke<StartComparisonResult>('start_comparison', { request });
  }

  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    _signal?: AbortSignal,
  ): Promise<ComparisonPage> {
    return invoke<ComparisonPage>('get_comparison', {
      comparisonId,
      offset: options?.offset,
      limit: options?.limit,
      differencesOnly: options?.differencesOnly,
    });
  }

  async cancelComparison(comparisonId: string, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_comparison', { comparisonId });
  }

  startChecksums(
    request: StartChecksumRequest,
    _signal?: AbortSignal,
  ): Promise<StartChecksumResult> {
    return invoke<StartChecksumResult>('start_checksums', { request });
  }

  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    _signal?: AbortSignal,
  ): Promise<ChecksumPage> {
    return invoke<ChecksumPage>('get_checksums', {
      jobId,
      offset: options?.offset,
      limit: options?.limit,
    });
  }

  async cancelChecksums(jobId: string, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_checksums', { jobId });
  }

  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    _signal?: AbortSignal,
  ): Promise<ChecksumFile> {
    return invoke<ChecksumFile>('render_checksum_file', { jobId, request: { algorithm } });
  }

  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    _signal?: AbortSignal,
  ): Promise<SavedChecksumFile> {
    return invoke<SavedChecksumFile>('save_checksum_file', { jobId, request });
  }

  verifyChecksumFile(
    jobId: string,
    content: string,
    _signal?: AbortSignal,
  ): Promise<VerificationReport> {
    return invoke<VerificationReport>('verify_checksum_file', { jobId, request: { content } });
  }

  startDuplicateScan(
    request: StartDuplicateScanRequest,
    _signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult> {
    return invoke<StartDuplicateScanResult>('start_duplicate_scan', { request });
  }

  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    _signal?: AbortSignal,
  ): Promise<DuplicatePage> {
    return invoke<DuplicatePage>('get_duplicate_scan', {
      scanId,
      offset: options?.offset,
      limit: options?.limit,
    });
  }

  async cancelDuplicateScan(scanId: string, _signal?: AbortSignal): Promise<void> {
    await invoke('cancel_duplicate_scan', { scanId });
  }

  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    _signal?: AbortSignal,
  ): Promise<SyncPlan> {
    return invoke<SyncPlan>('generate_sync_plan', { comparisonId, request });
  }

  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    _signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult> {
    return invoke<ApplySyncPlanResult>('apply_sync_plan', {
      comparisonId,
      request: { items: request.items.map(syncPlanItemToDto) },
    });
  }

  listActions(_signal?: AbortSignal): Promise<ActionDescriptor[]> {
    return invoke<ActionDescriptor[]>('list_actions');
  }

  invokeAction(request: InvokeActionRequest, _signal?: AbortSignal): Promise<ActionResult> {
    return invoke<ActionResult>('invoke_action', {
      actionId: request.actionId,
      request: { parameters: request.parameters, context: request.context },
    });
  }

  listPlugins(_signal?: AbortSignal): Promise<PluginDescriptor[]> {
    return invoke<PluginDescriptor[]>('list_plugins');
  }

  async setPluginEnabled(
    pluginId: PluginId,
    enabled: boolean,
    _signal?: AbortSignal,
  ): Promise<void> {
    await invoke(enabled ? 'enable_plugin' : 'disable_plugin', { pluginId });
  }

  getPluginLogs(pluginId: PluginId, _signal?: AbortSignal): Promise<PluginLogEntry[]> {
    return invoke<PluginLogEntry[]>('get_plugin_logs', { pluginId });
  }

  getPluginIconThemeAsset(
    pluginId: PluginId,
    assetPath: string,
    _signal?: AbortSignal,
  ): Promise<string> {
    return invoke<string>('get_plugin_icon_theme_asset', { pluginId, path: assetPath });
  }

  /** TODO(0034): full EventBus → Tauri channel parity; connects the minimal skeleton for now. */
  async subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    const unsubscribeListener = this.eventStream.listeners.subscribe(listener);
    await this.eventStream.connect();
    return () => {
      unsubscribeListener();
    };
  }

  onResynchronise(listener: () => void): Unsubscribe {
    return this.eventStream.resynchronise.subscribe(listener);
  }

  disconnect(): void {
    this.eventStream.close();
  }

  listConnections(_signal?: AbortSignal): Promise<Connection[]> {
    return invoke<Connection[]>('list_connections');
  }

  createConnection(request: CreateConnectionRequest, _signal?: AbortSignal): Promise<Connection> {
    return invoke<Connection>('create_connection', { request });
  }

  getConnection(connectionId: ConnectionId, _signal?: AbortSignal): Promise<Connection> {
    return invoke<Connection>('get_connection', { connectionId });
  }

  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    _signal?: AbortSignal,
  ): Promise<Connection> {
    return invoke<Connection>('update_connection', { connectionId, request });
  }

  async deleteConnection(connectionId: ConnectionId, _signal?: AbortSignal): Promise<void> {
    await invoke('delete_connection', { connectionId });
  }

  connectConnection(connectionId: ConnectionId, _signal?: AbortSignal): Promise<Connection> {
    return invoke<Connection>('connect_connection', { connectionId });
  }

  disconnectConnection(connectionId: ConnectionId, _signal?: AbortSignal): Promise<Connection> {
    return invoke<Connection>('disconnect_connection', { connectionId });
  }

  testConnection(connectionId: ConnectionId, _signal?: AbortSignal): Promise<Connection> {
    return invoke<Connection>('test_connection', { connectionId });
  }

  async beginOneDriveAuthorization(
    connectionId: ConnectionId,
    _signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse> {
    const response = await invoke<BeginOneDriveAuthorizationResponse>(
      'begin_onedrive_authorization',
      { connectionId },
    );
    await openUrl(trustedOneDriveAuthorizationUrl(response.authorizationUrl));
    return response;
  }

  getOneDriveAuthorizationAttempt(
    attemptId: string,
    _signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return invoke<OneDriveAuthorizationAttempt>('get_onedrive_authorization_attempt', {
      attemptId,
    });
  }

  cancelOneDriveAuthorization(
    attemptId: string,
    _signal?: AbortSignal,
  ): Promise<OneDriveAuthorizationAttempt> {
    return invoke<OneDriveAuthorizationAttempt>('cancel_onedrive_authorization', { attemptId });
  }

  probeSshHostKey(connectionId: ConnectionId, _signal?: AbortSignal): Promise<HostKeyProbe> {
    return invoke<HostKeyProbe>('probe_ssh_host_key', { connectionId });
  }

  async acceptSshHostKey(
    connectionId: ConnectionId,
    fingerprint: string,
    _signal?: AbortSignal,
  ): Promise<void> {
    await invoke('accept_ssh_host_key', { connectionId, request: { fingerprint } });
  }
}
