import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiError } from '../fetch-mutator';

const getRuntimeCapabilities = vi.fn();
const getSystemLocations = vi.fn();
const getVolumes = vi.fn();
const listDirectory = vi.fn();
const listDirectoryChildren = vi.fn();
const navigatePane = vi.fn();
const getEntryMetadata = vi.fn();
const listWorkspaces = vi.fn();
const createWorkspace = vi.fn();
const getWorkspace = vi.fn();
const deleteWorkspace = vi.fn();
const openWorkspace = vi.fn();
const applyWorkspaceCommand = vi.fn();
const requestStartOperation = vi.fn();
const requestListOperations = vi.fn();
const requestCancelOperation = vi.fn();
const requestUndoOperation = vi.fn();
const requestPauseOperation = vi.fn();
const requestResumeOperation = vi.fn();
const requestResolveOperationConflict = vi.fn();
const requestStartSearch = vi.fn();
const requestCancelSearch = vi.fn();
const requestStartComparison = vi.fn();
const requestGetComparison = vi.fn();
const requestCancelComparison = vi.fn();
const requestGenerateSyncPlan = vi.fn();
const requestApplySyncPlan = vi.fn();
const requestListPlugins = vi.fn();
const requestEnablePlugin = vi.fn();
const requestDisablePlugin = vi.fn();
const requestGetPluginLogs = vi.fn();
const requestReadFileRange = vi.fn();
const requestOpenDocxPreview = vi.fn();
const requestReadDocxPreviewResource = vi.fn();
const requestCloseDocxPreview = vi.fn();
const requestOpenPptxPreview = vi.fn();
const requestReadPptxPreviewPdf = vi.fn();
const requestClosePptxPreview = vi.fn();
const requestSearchInFile = vi.fn();
const requestOpenStructuredView = vi.fn();
const requestStructuredViewStatus = vi.fn();
const requestUpdateStructuredView = vi.fn();
const requestReadStructuredRows = vi.fn();
const requestReadStructuredJsonWindow = vi.fn();
const requestSearchStructuredRows = vi.fn();
const requestCloseStructuredView = vi.fn();
const requestGetFileIcon = vi.fn();
const requestDiscoverApplicationUninstallCandidates = vi.fn();
const requestRemoveApplicationDockIcon = vi.fn();
const requestBeginOneDriveAuthorization = vi.fn();
const requestGetOneDriveAuthorizationAttempt = vi.fn();
const requestCancelOneDriveAuthorization = vi.fn();

vi.mock('../generated/file-manager-api', () => ({
  getRuntimeCapabilities: (...args: unknown[]) => getRuntimeCapabilities(...args),
  getSystemLocations: (...args: unknown[]) => getSystemLocations(...args),
  getVolumes: (...args: unknown[]) => getVolumes(...args),
  listDirectory: (...args: unknown[]) => listDirectory(...args),
  listDirectoryChildren: (...args: unknown[]) => listDirectoryChildren(...args),
  navigatePane: (...args: unknown[]) => navigatePane(...args),
  getEntryMetadata: (...args: unknown[]) => getEntryMetadata(...args),
  listWorkspaces: (...args: unknown[]) => listWorkspaces(...args),
  createWorkspace: (...args: unknown[]) => createWorkspace(...args),
  getWorkspace: (...args: unknown[]) => getWorkspace(...args),
  deleteWorkspace: (...args: unknown[]) => deleteWorkspace(...args),
  openWorkspace: (...args: unknown[]) => openWorkspace(...args),
  applyWorkspaceCommand: (...args: unknown[]) => applyWorkspaceCommand(...args),
  startOperation: (...args: unknown[]) => requestStartOperation(...args),
  listOperations: (...args: unknown[]) => requestListOperations(...args),
  cancelOperation: (...args: unknown[]) => requestCancelOperation(...args),
  undoOperation: (...args: unknown[]) => requestUndoOperation(...args),
  pauseOperation: (...args: unknown[]) => requestPauseOperation(...args),
  resumeOperation: (...args: unknown[]) => requestResumeOperation(...args),
  resolveOperationConflict: (...args: unknown[]) => requestResolveOperationConflict(...args),
  startSearch: (...args: unknown[]) => requestStartSearch(...args),
  cancelSearch: (...args: unknown[]) => requestCancelSearch(...args),
  startComparison: (...args: unknown[]) => requestStartComparison(...args),
  getComparison: (...args: unknown[]) => requestGetComparison(...args),
  cancelComparison: (...args: unknown[]) => requestCancelComparison(...args),
  generateSyncPlan: (...args: unknown[]) => requestGenerateSyncPlan(...args),
  applySyncPlan: (...args: unknown[]) => requestApplySyncPlan(...args),
  listPlugins: (...args: unknown[]) => requestListPlugins(...args),
  enablePlugin: (...args: unknown[]) => requestEnablePlugin(...args),
  disablePlugin: (...args: unknown[]) => requestDisablePlugin(...args),
  getPluginLogs: (...args: unknown[]) => requestGetPluginLogs(...args),
  readFileRange: (...args: unknown[]) => requestReadFileRange(...args),
  openDocxPreview: (...args: unknown[]) => requestOpenDocxPreview(...args),
  readDocxPreviewResource: (...args: unknown[]) => requestReadDocxPreviewResource(...args),
  closeDocxPreview: (...args: unknown[]) => requestCloseDocxPreview(...args),
  openPptxPreview: (...args: unknown[]) => requestOpenPptxPreview(...args),
  readPptxPreviewPdf: (...args: unknown[]) => requestReadPptxPreviewPdf(...args),
  closePptxPreview: (...args: unknown[]) => requestClosePptxPreview(...args),
  searchInFile: (...args: unknown[]) => requestSearchInFile(...args),
  openStructuredView: (...args: unknown[]) => requestOpenStructuredView(...args),
  getStructuredViewStatus: (...args: unknown[]) => requestStructuredViewStatus(...args),
  updateStructuredView: (...args: unknown[]) => requestUpdateStructuredView(...args),
  readStructuredRows: (...args: unknown[]) => requestReadStructuredRows(...args),
  readStructuredJsonWindow: (...args: unknown[]) => requestReadStructuredJsonWindow(...args),
  searchStructuredRows: (...args: unknown[]) => requestSearchStructuredRows(...args),
  closeStructuredView: (...args: unknown[]) => requestCloseStructuredView(...args),
  getFileIcon: (...args: unknown[]) => requestGetFileIcon(...args),
  discoverApplicationUninstallCandidates: (...args: unknown[]) =>
    requestDiscoverApplicationUninstallCandidates(...args),
  removeApplicationDockIcon: (...args: unknown[]) => requestRemoveApplicationDockIcon(...args),
  beginOneDriveAuthorization: (...args: unknown[]) => requestBeginOneDriveAuthorization(...args),
  getOneDriveAuthorizationAttempt: (...args: unknown[]) =>
    requestGetOneDriveAuthorizationAttempt(...args),
  cancelOneDriveAuthorization: (...args: unknown[]) => requestCancelOneDriveAuthorization(...args),
}));

const { HttpFileManagerClient } = await import('./http-file-manager-client');

class TestEventSource extends EventTarget {
  close(): void {}
}

beforeEach(() => {
  vi.stubGlobal('EventSource', TestEventSource);
});

function fixtureCapabilities() {
  return {
    clipboard: true,
    extendedAttributes: false,
    finderAliases: false,
    finderTags: false,
    nativeDragOut: false,
    nativeFileIcons: false,
    nativeMenus: false,
    platformContextMenu: false,
    nativeThumbnails: false,
    openTerminal: false,
    platform: 'macos',
    plugins: false,
    revealInSystemFileManager: true,
    runtime: 'browserServer',
    serverAdministration: false,
    systemTrash: true,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  getRuntimeCapabilities.mockReset();
  listDirectory.mockReset();
  listDirectoryChildren.mockReset();
  navigatePane.mockReset();
  getEntryMetadata.mockReset();
  listWorkspaces.mockReset();
  createWorkspace.mockReset();
  getWorkspace.mockReset();
  deleteWorkspace.mockReset();
  openWorkspace.mockReset();
  applyWorkspaceCommand.mockReset();
  requestStartOperation.mockReset();
  requestListOperations.mockReset();
  requestCancelOperation.mockReset();
  requestUndoOperation.mockReset();
  requestPauseOperation.mockReset();
  requestResumeOperation.mockReset();
  requestResolveOperationConflict.mockReset();
  requestStartSearch.mockReset();
  requestCancelSearch.mockReset();
  requestStartComparison.mockReset();
  requestGetComparison.mockReset();
  requestCancelComparison.mockReset();
  requestGenerateSyncPlan.mockReset();
  requestApplySyncPlan.mockReset();
  requestListPlugins.mockReset();
  requestEnablePlugin.mockReset();
  requestDisablePlugin.mockReset();
  requestGetPluginLogs.mockReset();
  requestReadFileRange.mockReset();
  requestSearchInFile.mockReset();
  requestOpenStructuredView.mockReset();
  requestStructuredViewStatus.mockReset();
  requestUpdateStructuredView.mockReset();
  requestReadStructuredRows.mockReset();
  requestReadStructuredJsonWindow.mockReset();
  requestSearchStructuredRows.mockReset();
  requestCloseStructuredView.mockReset();
  requestGetFileIcon.mockReset();
  requestBeginOneDriveAuthorization.mockReset();
  requestGetOneDriveAuthorizationAttempt.mockReset();
  requestCancelOneDriveAuthorization.mockReset();
});

describe('HttpFileManagerClient', () => {
  describe('OneDrive authorization', () => {
    it('opens a placeholder synchronously, then navigates it to Microsoft after begin succeeds', async () => {
      const popup = { location: { href: '' }, close: vi.fn(), opener: window };
      const open = vi.fn().mockReturnValue(popup);
      vi.stubGlobal('open', open);
      requestBeginOneDriveAuthorization.mockResolvedValue({
        status: 201,
        data: {
          attemptId: 'attempt-1',
          authorizationUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
        },
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();

      const pending = client.beginOneDriveAuthorization('connection-1');

      expect(open).toHaveBeenCalledWith('', '_blank');
      expect(popup.opener).toBeNull();
      await expect(pending).resolves.toEqual({
        attemptId: 'attempt-1',
        authorizationUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
      });
      expect(popup.location.href).toBe(
        'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
      );
    });

    it('closes the placeholder if beginning authorization fails', async () => {
      const popup = { location: { href: '' }, close: vi.fn(), opener: window };
      vi.stubGlobal('open', vi.fn().mockReturnValue(popup));
      requestBeginOneDriveAuthorization.mockRejectedValue(new Error('offline'));

      await expect(
        new HttpFileManagerClient().beginOneDriveAuthorization('connection-1'),
      ).rejects.toThrow('offline');
      expect(popup.close).toHaveBeenCalledOnce();
    });

    it('rejects an authorization URL outside the expected Microsoft endpoint', async () => {
      const popup = { location: { href: '' }, close: vi.fn(), opener: window };
      vi.stubGlobal('open', vi.fn().mockReturnValue(popup));
      requestBeginOneDriveAuthorization.mockResolvedValue({
        status: 201,
        data: { attemptId: 'attempt-1', authorizationUrl: 'javascript:alert(document.cookie)' },
        headers: new Headers(),
      });

      await expect(
        new HttpFileManagerClient().beginOneDriveAuthorization('connection-1'),
      ).rejects.toThrow('Microsoft authorization URL');
      expect(popup.location.href).toBe('');
      expect(popup.close).toHaveBeenCalledOnce();
    });

    it('polls and cancels through the generated HTTP endpoints', async () => {
      const attempt = { id: 'attempt-1', status: { state: 'pending' as const } };
      requestGetOneDriveAuthorizationAttempt.mockResolvedValue({
        status: 200,
        data: attempt,
        headers: new Headers(),
      });
      requestCancelOneDriveAuthorization.mockResolvedValue({
        status: 200,
        data: { ...attempt, status: { state: 'cancelled' as const } },
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();

      await expect(client.getOneDriveAuthorizationAttempt('attempt-1')).resolves.toEqual(attempt);
      await expect(client.cancelOneDriveAuthorization('attempt-1')).resolves.toEqual({
        ...attempt,
        status: { state: 'cancelled' },
      });
    });
  });

  describe('getFileIcon', () => {
    it('returns binary icon bytes and forwards cancellation', async () => {
      const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
      requestGetFileIcon.mockResolvedValue({
        status: 200,
        data: new Blob([bytes], { type: 'image/png' }),
        headers: new Headers({ 'content-type': 'image/png' }),
      });
      const controller = new AbortController();
      const client = new HttpFileManagerClient();

      await expect(client.getFileIcon('file:///report.pdf', controller.signal)).resolves.toEqual(
        bytes,
      );
      expect(requestGetFileIcon).toHaveBeenCalledWith(
        { uri: 'file:///report.pdf' },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('silently returns undefined for unsupported hosts and fetch failures', async () => {
      requestGetFileIcon.mockRejectedValue(new Error('not found'));
      const client = new HttpFileManagerClient();

      await expect(client.getFileIcon('file:///report.pdf')).resolves.toBeUndefined();
    });
  });

  describe('getRuntimeCapabilities', () => {
    it('maps the generated client response data to the frontend model (happy path)', async () => {
      const fixture = fixtureCapabilities();
      getRuntimeCapabilities.mockResolvedValue({
        status: 200,
        data: fixture,
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();

      const result = await client.getRuntimeCapabilities();

      expect(result).toEqual(fixture);
    });

    it('forwards the caller-provided AbortSignal to the generated client call', async () => {
      getRuntimeCapabilities.mockResolvedValue({
        status: 200,
        data: fixtureCapabilities(),
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await client.getRuntimeCapabilities(controller.signal);

      expect(getRuntimeCapabilities).toHaveBeenCalledWith(
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('propagates a rejected ApiError without wrapping or leaking a raw Response', async () => {
      const apiError = new ApiError(500, { code: 'unknownError', message: 'boom' });
      getRuntimeCapabilities.mockRejectedValue(apiError);
      const client = new HttpFileManagerClient();

      await expect(client.getRuntimeCapabilities()).rejects.toBe(apiError);
    });

    it('propagates an abort rejection rather than swallowing the cancellation', async () => {
      const abortError = new DOMException('The operation was aborted.', 'AbortError');
      getRuntimeCapabilities.mockRejectedValue(abortError);
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await expect(client.getRuntimeCapabilities(controller.signal)).rejects.toBe(abortError);
    });
  });

  describe('getSystemLocations', () => {
    it('maps discovered locations and forwards cancellation', async () => {
      getSystemLocations.mockResolvedValue({
        status: 200,
        data: [
          {
            name: 'Team Files',
            kind: 'network',
            location: { providerId: 'local', uri: 'file:///Example' },
            protocol: 'smb',
            server: 'files.example.test',
            share: 'team',
            readOnly: true,
          },
        ],
        headers: new Headers(),
      });
      const controller = new AbortController();

      await expect(
        new HttpFileManagerClient().getSystemLocations(controller.signal),
      ).resolves.toEqual([
        {
          name: 'Team Files',
          kind: 'network',
          location: { providerId: 'local', uri: 'file:///Example' },
          protocol: 'smb',
          server: 'files.example.test',
          share: 'team',
          readOnly: true,
        },
      ]);
      expect(getSystemLocations).toHaveBeenCalledWith(
        expect.objectContaining({ signal: controller.signal }),
      );
    });
  });

  describe('getVolumes', () => {
    it('maps discovered volumes and forwards cancellation', async () => {
      getVolumes.mockResolvedValue({
        status: 200,
        data: [{ name: 'Macintosh HD', location: { providerId: 'local', uri: 'file:///' } }],
        headers: new Headers(),
      });
      const controller = new AbortController();

      await expect(new HttpFileManagerClient().getVolumes(controller.signal)).resolves.toEqual([
        { name: 'Macintosh HD', location: { providerId: 'local', uri: 'file:///' } },
      ]);
      expect(getVolumes).toHaveBeenCalledWith(
        expect.objectContaining({ signal: controller.signal }),
      );
    });
  });

  describe('subscribe', () => {
    it('connects the shared SSE stream and returns its listener unsubscribe', async () => {
      const client = new HttpFileManagerClient();

      const unsubscribe = await client.subscribe(() => {});

      expect(() => unsubscribe()).not.toThrow();
      expect(client.connection.get()).toBe('connecting');
      client.disconnect();
      expect(client.connection.get()).toBe('closed');
    });
  });

  describe('directory methods', () => {
    it('calls the generated list endpoint and forwards cancellation', async () => {
      const snapshot = {
        paneId: 'pane-1',
        requestId: 'req-1',
        revision: 1,
        location: { providerId: 'local', uri: 'file:///' },
        entries: [],
        hasMore: false,
        loadingState: { type: 'loaded' },
      };
      listDirectory.mockResolvedValue({ status: 200, data: snapshot, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const request = {
        workspaceId: 'workspace-1',
        paneId: 'pane-1',
        requestId: 'req-1',
        location: { providerId: 'local', uri: 'file:///' },
      };

      await expect(client.listDirectory(request, controller.signal)).resolves.toEqual(snapshot);
      expect(listDirectory).toHaveBeenCalledWith(
        request,
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('calls the generated children endpoint and normalizes the returned entries', async () => {
      const dto = [
        {
          id: 'entry-1',
          location: { providerId: 'local', uri: 'file:///child' },
          name: 'child',
          kind: 'directory',
          hidden: false,
          readOnly: false,
          metadataRevision: 1,
        },
      ];
      listDirectoryChildren.mockResolvedValue({ status: 200, data: dto, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const location = { providerId: 'local', uri: 'file:///' };

      const children = await client.listDirectoryChildren(location, false, controller.signal);

      expect(children).toEqual([
        {
          id: 'entry-1',
          location: { providerId: 'local', uri: 'file:///child' },
          name: 'child',
          kind: 'directory',
          hidden: false,
          readOnly: false,
          metadataRevision: 1,
        },
      ]);
      expect(listDirectoryChildren).toHaveBeenCalledWith(
        { location, showHidden: false },
        expect.objectContaining({ signal: controller.signal }),
      );
    });
  });

  describe('workspace methods', () => {
    it('normalizes workspace DTOs returned by semantic command dispatch', async () => {
      const dto = {
        id: 'workspace-1',
        name: 'Renamed',
        revision: 2,
        layout: { type: 'pane', paneId: 'pane-1' },
        panes: [],
        activePaneId: 'pane-1',
        operationCentre: { visible: false, height: 180 },
      };
      applyWorkspaceCommand.mockResolvedValue({ status: 200, data: dto, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const command = {
        type: 'renameWorkspace',
        workspaceId: 'workspace-1',
        expectedRevision: 1,
        name: 'Renamed',
      } as const;

      await expect(client.dispatchWorkspaceCommand(command)).resolves.toEqual(
        expect.objectContaining({
          id: 'workspace-1',
          name: 'Renamed',
          paneOrder: [],
          panesById: {},
        }),
      );
      expect(applyWorkspaceCommand).toHaveBeenCalledWith('workspace-1', command, undefined);
    });
  });

  describe('operation methods', () => {
    it('starts a semantic operation and maps the wire type discriminator', async () => {
      requestStartOperation.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: {
          id: 'operation-1',
          type: 'copy',
          state: 'queued',
          sources: [],
          destination: null,
          progress: { completedItems: 0, completedBytes: 0 },
          conflictPolicy: 'ask',
          createdAt: '2026-07-31T12:00:00Z',
          errors: [],
          undo: { available: false, reason: 'Operation is still running.' },
        },
      });
      const client = new HttpFileManagerClient();
      const request = {
        type: 'copy',
        sources: [{ providerId: 'local', uri: 'file:///Documents' }],
        conflictPolicy: 'ask',
      } as const;

      await expect(client.startOperation(request)).resolves.toMatchObject({
        id: 'operation-1',
        kind: 'copy',
        state: 'queued',
      });
      expect(requestStartOperation).toHaveBeenCalledWith(request, undefined);
    });

    it('lists operations and forwards cancellation to every lifecycle request', async () => {
      requestListOperations.mockResolvedValue({
        status: 200,
        data: { operations: [] },
        headers: new Headers(),
      });

      requestCancelOperation.mockResolvedValue({ status: 204, headers: new Headers() });
      requestPauseOperation.mockResolvedValue({ status: 204, headers: new Headers() });
      requestResumeOperation.mockResolvedValue({ status: 204, headers: new Headers() });
      const controller = new AbortController();
      const client = new HttpFileManagerClient();

      await expect(client.listOperations(controller.signal)).resolves.toEqual([]);
      await client.cancelOperation('operation-1', controller.signal);
      await client.pauseOperation('operation-1', controller.signal);
      await client.resumeOperation('operation-1', controller.signal);

      const options = expect.objectContaining({ signal: controller.signal });
      expect(requestListOperations).toHaveBeenCalledWith(undefined, options);
      expect(requestCancelOperation).toHaveBeenCalledWith('operation-1', options);
      expect(requestPauseOperation).toHaveBeenCalledWith('operation-1', options);
      expect(requestResumeOperation).toHaveBeenCalledWith('operation-1', options);
    });

    it('starts undo through the generated operation endpoint', async () => {
      requestUndoOperation.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: {
          id: 'undo-1',
          type: 'undo',
          state: 'queued',
          sources: [],
          destination: null,
          progress: { completedItems: 0, completedBytes: 0 },
          conflictPolicy: 'ask',
          createdAt: '2026-08-31T12:00:00Z',
          errors: [],
          undo: { available: false, reason: 'Undo operations cannot themselves be undone.' },
          undoOf: 'operation-1',
        },
      });
      const client = new HttpFileManagerClient();

      await expect(client.undoOperation('operation-1')).resolves.toMatchObject({
        id: 'undo-1',
        kind: 'undo',
        undoOf: 'operation-1',
      });
      expect(requestUndoOperation).toHaveBeenCalledWith('operation-1', undefined);
    });

    it('reserves the exact conflict request shape without duplicating the operation id in JSON', async () => {
      requestResolveOperationConflict.mockResolvedValue({
        status: 204,
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();

      await client.resolveConflict({
        operationId: 'operation-1',
        resolution: 'renameNew',
        applyToAllSimilar: true,
      });

      expect(requestResolveOperationConflict).toHaveBeenCalledWith(
        'operation-1',
        { resolution: 'renameNew', applyToAllSimilar: true },
        undefined,
      );
    });
  });

  describe('search methods', () => {
    it('starts a filename search and returns its id and virtual location', async () => {
      requestStartSearch.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: {
          searchId: 'search-1',
          location: { providerId: 'local', uri: 'search://local/search-1' },
          limitations: [],
          executionMode: 'indexed',
        },
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await expect(
        client.startSearch(
          {
            query: 'report',
            roots: [{ providerId: 'local', uri: 'file:///Documents' }],
            workspaceId: 'workspace-1',
          },
          controller.signal,
        ),
      ).resolves.toEqual({
        searchId: 'search-1',
        location: { providerId: 'local', uri: 'search://local/search-1' },
        limitations: [],
        executionMode: 'indexed',
      });
      expect(requestStartSearch).toHaveBeenCalledWith(
        {
          query: 'report',
          roots: [{ providerId: 'local', uri: 'file:///Documents' }],
          workspaceId: 'workspace-1',
        },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected startSearch response status', async () => {
      requestStartSearch.mockResolvedValue({ status: 400, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(
        client.startSearch({ query: 'x', roots: [], workspaceId: 'workspace-1' }),
      ).rejects.toThrow('Unexpected startSearch response status: 400');
    });

    it('forwards the content-search fields to the backend (regression: these were silently dropped)', async () => {
      requestStartSearch.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: {
          searchId: 'search-1',
          location: { providerId: 'local', uri: 'search://local/search-1' },
          limitations: [],
          executionMode: 'liveRecursive',
        },
      });
      const client = new HttpFileManagerClient();

      await client.startSearch({
        query: '*.md',
        contentQuery: 'archive',
        contentRegex: false,
        contentCaseSensitive: false,
        contentWholeWord: true,
        recurse: true,
        roots: [{ providerId: 'local', uri: 'file:///Documents' }],
        workspaceId: 'workspace-1',
      });

      expect(requestStartSearch).toHaveBeenCalledWith(
        expect.objectContaining({
          contentQuery: 'archive',
          contentRegex: false,
          contentCaseSensitive: false,
          contentWholeWord: true,
          recurse: true,
        }),
        undefined,
      );
    });

    it('forwards showHidden to the backend so hidden files are excluded when show-hidden is off', async () => {
      requestStartSearch.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: {
          searchId: 'search-1',
          location: { providerId: 'local', uri: 'search://local/search-1' },
          limitations: [],
          executionMode: 'liveRecursive',
        },
      });
      const client = new HttpFileManagerClient();

      await client.startSearch({
        query: '*.md',
        recurse: true,
        showHidden: false,
        roots: [{ providerId: 'local', uri: 'file:///Documents' }],
        workspaceId: 'workspace-1',
      });

      expect(requestStartSearch).toHaveBeenCalledWith(
        expect.objectContaining({ showHidden: false }),
        undefined,
      );
    });

    it('cancels a search', async () => {
      requestCancelSearch.mockResolvedValue({ status: 204, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await client.cancelSearch('search-1', controller.signal);

      expect(requestCancelSearch).toHaveBeenCalledWith(
        'search-1',
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected cancelSearch response status', async () => {
      requestCancelSearch.mockResolvedValue({ status: 404, headers: new Headers() });
      const client = new HttpFileManagerClient();

      await expect(client.cancelSearch('search-1')).rejects.toThrow(
        'Unexpected cancelSearch response status: 404',
      );
    });
  });

  describe('comparison methods', () => {
    it('starts a comparison and returns its id', async () => {
      requestStartComparison.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: { comparisonId: 'comparison-1' },
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await expect(
        client.startComparison(
          {
            workspaceId: 'workspace-1',
            left: { providerId: 'local', uri: 'file:///left' },
            right: { providerId: 'local', uri: 'file:///right' },
            criteria: 'sizeAndTimestamp',
            showHidden: true,
          },
          controller.signal,
        ),
      ).resolves.toEqual({ comparisonId: 'comparison-1' });
      expect(requestStartComparison).toHaveBeenCalledWith(
        {
          workspaceId: 'workspace-1',
          left: { providerId: 'local', uri: 'file:///left' },
          right: { providerId: 'local', uri: 'file:///right' },
          criteria: 'sizeAndTimestamp',
          showHidden: true,
        },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected startComparison response status', async () => {
      requestStartComparison.mockResolvedValue({ status: 400, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(
        client.startComparison({
          workspaceId: 'workspace-1',
          left: { providerId: 'local', uri: 'file:///left' },
          right: { providerId: 'local', uri: 'file:///right' },
          criteria: 'nameOnly',
        }),
      ).rejects.toThrow('Unexpected startComparison response status: 400');
    });

    it('pages a comparison, normalizing null DTO fields and forwarding filter params', async () => {
      requestGetComparison.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: {
          comparisonId: 'comparison-1',
          left: { providerId: 'local', uri: 'file:///left' },
          right: { providerId: 'local', uri: 'file:///right' },
          criteria: 'nameOnly',
          offset: 0,
          limit: 200,
          total: 1,
          isComplete: true,
          warningsCount: 0,
          entries: [
            {
              relativePath: 'a.txt',
              status: 'onlyLeft',
              left: { kind: 'file', size: 10, modifiedAt: null, contentHash: null },
              right: null,
            },
          ],
        },
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      const page = await client.getComparison(
        'comparison-1',
        { offset: 5, limit: 50, differencesOnly: true },
        controller.signal,
      );

      expect(page.entries).toEqual([
        { relativePath: 'a.txt', status: 'onlyLeft', left: { kind: 'file', size: 10 } },
      ]);
      expect(requestGetComparison).toHaveBeenCalledWith(
        'comparison-1',
        { offset: 5, limit: 50, differencesOnly: true },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected getComparison response status', async () => {
      requestGetComparison.mockResolvedValue({ status: 404, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(client.getComparison('comparison-1')).rejects.toThrow(
        'Unexpected getComparison response status: 404',
      );
    });

    it('cancels a comparison', async () => {
      requestCancelComparison.mockResolvedValue({ status: 204, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();

      await client.cancelComparison('comparison-1', controller.signal);

      expect(requestCancelComparison).toHaveBeenCalledWith(
        'comparison-1',
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected cancelComparison response status', async () => {
      requestCancelComparison.mockResolvedValue({ status: 404, headers: new Headers() });
      const client = new HttpFileManagerClient();

      await expect(client.cancelComparison('comparison-1')).rejects.toThrow(
        'Unexpected cancelComparison response status: 404',
      );
    });

    it('generates a sync plan', async () => {
      requestGenerateSyncPlan.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: {
          comparisonId: 'comparison-1',
          items: [
            {
              relativePath: 'a.txt',
              status: 'onlyLeft',
              action: 'copyLeftToRight',
              left: { kind: 'file', size: 10, modifiedAt: null, contentHash: null },
              right: null,
            },
          ],
        },
      });
      const client = new HttpFileManagerClient();

      const plan = await client.generateSyncPlan('comparison-1', { mode: 'mirrorLeftToRight' });

      expect(plan.items).toEqual([
        {
          relativePath: 'a.txt',
          status: 'onlyLeft',
          action: 'copyLeftToRight',
          left: { kind: 'file', size: 10 },
        },
      ]);
      expect(requestGenerateSyncPlan).toHaveBeenCalledWith(
        'comparison-1',
        { mode: 'mirrorLeftToRight' },
        undefined,
      );
    });

    it('applies a sync plan, omitting undefined sides from the wire request', async () => {
      requestApplySyncPlan.mockResolvedValue({
        status: 201,
        headers: new Headers(),
        data: { operationIds: ['operation-1'] },
      });
      const client = new HttpFileManagerClient();

      const result = await client.applySyncPlan('comparison-1', {
        items: [
          { relativePath: 'a.txt', status: 'onlyLeft', action: 'copyLeftToRight' },
          { relativePath: 'b.txt', status: 'identical', action: 'skip' },
        ],
      });

      expect(result).toEqual({ operationIds: ['operation-1'] });
      expect(requestApplySyncPlan).toHaveBeenCalledWith(
        'comparison-1',
        {
          items: [
            { relativePath: 'a.txt', status: 'onlyLeft', action: 'copyLeftToRight' },
            { relativePath: 'b.txt', status: 'identical', action: 'skip' },
          ],
        },
        undefined,
      );
    });

    it('rejects an unexpected applySyncPlan response status', async () => {
      requestApplySyncPlan.mockResolvedValue({ status: 400, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(client.applySyncPlan('comparison-1', { items: [] })).rejects.toThrow(
        'Unexpected applySyncPlan response status: 400',
      );
    });
  });

  describe('file range and content search methods', () => {
    it('uses the same structured-view session contract in the browser adapter', async () => {
      const view = {
        sessionId: 'structured-1',
        sourceRevision: 'revision-1',
        sourceBytes: 4_000_000_000,
        kind: 'table' as const,
        delimiter: ',',
        headerMode: 'present' as const,
        headers: ['name', 'value'],
        rows: [{ index: 0, cells: ['alpha', '1'] }],
        indexedBytes: 1024,
        indexedRows: 1,
        indexingComplete: false,
        randomAccess: true,
      };
      const status = { indexedBytes: 2048, indexedRows: 2, indexingComplete: false };
      const rows = {
        startRow: 1,
        rows: [{ index: 1, cells: ['beta', '2'] }],
        indexedRows: 2,
        indexingComplete: false,
      };
      const jsonWindow = {
        offset: 0,
        data: [123, 125],
        eof: true,
        tokens: [],
        indexedBytes: 2,
        indexingComplete: true,
      };
      const search = {
        rows: [{ index: 1, cells: ['beta', '2'] }],
        nextCursor: null,
        indexingComplete: false,
      };
      requestOpenStructuredView.mockResolvedValue({ status: 200, data: view });
      requestStructuredViewStatus.mockResolvedValue({ status: 200, data: status });
      requestUpdateStructuredView.mockResolvedValue({ status: 200, data: view });
      requestReadStructuredRows.mockResolvedValue({ status: 200, data: rows });
      requestReadStructuredJsonWindow.mockResolvedValue({ status: 200, data: jsonWindow });
      requestSearchStructuredRows.mockResolvedValue({ status: 200, data: search });
      requestCloseStructuredView.mockResolvedValue({ status: 204 });
      const client = new HttpFileManagerClient();
      const session = { sessionId: 'structured-1' };
      const open = {
        location: { providerId: 'local', uri: 'file:///large.csv' },
        format: 'csv' as const,
      };

      await expect(client.openStructuredView(open)).resolves.toEqual(view);
      await expect(client.getStructuredViewStatus(session)).resolves.toEqual(status);
      await expect(
        client.updateStructuredView({ ...session, selectedSheet: 'Details' }),
      ).resolves.toEqual(view);
      await expect(
        client.readStructuredRows({ ...session, startRow: 1, count: 200 }),
      ).resolves.toEqual(rows);
      await expect(
        client.readStructuredJsonWindow({ ...session, offset: 0, length: 65_536 }),
      ).resolves.toEqual(jsonWindow);
      await expect(
        client.searchStructuredRows({ ...session, query: 'beta', limit: 20 }),
      ).resolves.toEqual(search);
      await expect(client.closeStructuredView(session)).resolves.toBeUndefined();

      expect(requestOpenStructuredView).toHaveBeenCalledWith(open, undefined);
      expect(requestStructuredViewStatus).toHaveBeenCalledWith(session, undefined);
      expect(requestCloseStructuredView).toHaveBeenCalledWith(session, undefined);
    });

    it('reads a byte range from a file', async () => {
      const chunk = {
        data: [1, 2, 3],
        offset: 0,
        length: 3,
        eof: false,
        probablyBinary: false,
      };
      requestReadFileRange.mockResolvedValue({ status: 200, data: chunk, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const request = {
        location: { providerId: 'local', uri: 'file:///report.txt' },
        offset: 0,
        length: 3,
      };

      await expect(client.readFileRange(request, controller.signal)).resolves.toEqual(chunk);
      expect(requestReadFileRange).toHaveBeenCalledWith(
        request,
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected readFileRange response status', async () => {
      requestReadFileRange.mockResolvedValue({ status: 400, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(
        client.readFileRange({
          location: { providerId: 'local', uri: 'file:///report.txt' },
          offset: 0,
          length: 3,
        }),
      ).rejects.toThrow('Unexpected readFileRange response status: 400');
    });

    it('uses the generated DOCX session endpoints with the caller signal', async () => {
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const location = { providerId: 'local', uri: 'file:///report.docx' };
      const preview = {
        sessionId: 'docx-session',
        sourceRevision: 'r1',
        sourceBytes: 1024,
        html: '<p>Report</p>',
        resources: [],
        omittedFeatures: ['exact pagination'],
      };
      requestOpenDocxPreview.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: preview,
      });

      requestReadDocxPreviewResource.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: { data: [137, 80], mediaType: 'image/png' },
      });

      requestCloseDocxPreview.mockResolvedValue({
        status: 204,
        headers: new Headers(),
        data: undefined,
      });

      await expect(client.openDocxPreview({ location }, controller.signal)).resolves.toEqual(
        preview,
      );
      await expect(
        client.readDocxPreviewResource(
          { sessionId: 'docx-session', resourceId: 'image-1' },
          controller.signal,
        ),
      ).resolves.toEqual({ data: [137, 80], mediaType: 'image/png' });
      await expect(
        client.closeDocxPreview({ sessionId: 'docx-session' }, controller.signal),
      ).resolves.toBeUndefined();
      expect(requestOpenDocxPreview).toHaveBeenCalledWith(
        { location },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('uses the generated PPTX PDF session endpoints with the caller signal', async () => {
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const location = { providerId: 'local', uri: 'file:///briefing.pptx' };
      const preview = {
        sessionId: 'pptx-session',
        sourceRevision: 'r1',
        sourceBytes: 2048,
        firstPagePdf: [37, 80, 68, 70],
      };
      requestOpenPptxPreview.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: preview,
      });
      requestReadPptxPreviewPdf.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: { data: [37, 80, 68, 70], offset: 0, length: 4, eof: false },
      });
      requestClosePptxPreview.mockResolvedValue({
        status: 204,
        headers: new Headers(),
        data: undefined,
      });

      await expect(client.openPptxPreview({ location }, controller.signal)).resolves.toEqual(
        preview,
      );
      await expect(
        client.readPptxPreviewPdf(
          { sessionId: 'pptx-session', offset: 0, length: 4 },
          controller.signal,
        ),
      ).resolves.toEqual({ data: [37, 80, 68, 70], offset: 0, length: 4, eof: false });
      await expect(
        client.closePptxPreview({ sessionId: 'pptx-session' }, controller.signal),
      ).resolves.toBeUndefined();
      expect(requestOpenPptxPreview).toHaveBeenCalledWith(
        { location },
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('searches a file for content matches', async () => {
      const result = {
        matches: [{ lineNumber: 1, offset: 0, length: 5 }],
        truncated: false,
      };
      requestSearchInFile.mockResolvedValue({ status: 200, data: result, headers: new Headers() });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const request = {
        location: { providerId: 'local', uri: 'file:///report.txt' },
        query: 'error',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
      };

      await expect(client.searchInFile(request, controller.signal)).resolves.toEqual(result);
      expect(requestSearchInFile).toHaveBeenCalledWith(
        request,
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected searchInFile response status', async () => {
      requestSearchInFile.mockResolvedValue({ status: 400, headers: new Headers(), data: {} });
      const client = new HttpFileManagerClient();

      await expect(
        client.searchInFile({
          location: { providerId: 'local', uri: 'file:///report.txt' },
          query: 'error',
          regex: false,
          caseSensitive: false,
          wholeWord: false,
        }),
      ).rejects.toThrow('Unexpected searchInFile response status: 400');
    });
  });

  describe('discoverApplicationUninstallCandidates', () => {
    it("discovers an application bundle's related files", async () => {
      const result = {
        bundleIdentifier: 'com.example.Widget',
        productName: 'Widget',
        relatedFiles: [
          {
            location: { providerId: 'local', uri: 'file:///Users/erik/Library/Caches/Widget' },
            sizeBytes: 1024,
            removable: true,
          },
        ],
      };
      requestDiscoverApplicationUninstallCandidates.mockResolvedValue({
        status: 200,
        data: result,
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const request = { location: { providerId: 'local', uri: 'file:///Applications/Widget.app' } };

      await expect(
        client.discoverApplicationUninstallCandidates(request, controller.signal),
      ).resolves.toEqual(result);
      expect(requestDiscoverApplicationUninstallCandidates).toHaveBeenCalledWith(
        request,
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected discoverApplicationUninstallCandidates response status', async () => {
      requestDiscoverApplicationUninstallCandidates.mockResolvedValue({
        status: 404,
        headers: new Headers(),
        data: {},
      });
      const client = new HttpFileManagerClient();

      await expect(
        client.discoverApplicationUninstallCandidates({
          location: { providerId: 'local', uri: 'file:///Applications/Missing.app' },
        }),
      ).rejects.toThrow('Unexpected discoverApplicationUninstallCandidates response status: 404');
    });
  });

  describe('removeApplicationDockIcon', () => {
    it('reports whether a pinned Dock icon was found and removed', async () => {
      requestRemoveApplicationDockIcon.mockResolvedValue({
        status: 200,
        data: { removed: true },
        headers: new Headers(),
      });
      const client = new HttpFileManagerClient();
      const controller = new AbortController();
      const request = { location: { providerId: 'local', uri: 'file:///Applications/Widget.app' } };

      await expect(client.removeApplicationDockIcon(request, controller.signal)).resolves.toEqual({
        removed: true,
      });
      expect(requestRemoveApplicationDockIcon).toHaveBeenCalledWith(
        request,
        expect.objectContaining({ signal: controller.signal }),
      );
    });

    it('rejects an unexpected removeApplicationDockIcon response status', async () => {
      requestRemoveApplicationDockIcon.mockResolvedValue({
        status: 502,
        headers: new Headers(),
        data: {},
      });
      const client = new HttpFileManagerClient();

      await expect(
        client.removeApplicationDockIcon({
          location: { providerId: 'local', uri: 'file:///Applications/Widget.app' },
        }),
      ).rejects.toThrow('Unexpected removeApplicationDockIcon response status: 502');
    });
  });

  describe('plugin methods', () => {
    function fixturePermissions() {
      return {
        selectedEntryMetadata: true,
        selectedEntryContentRead: false,
        filesystemRead: [],
        filesystemWrite: [],
        clipboardRead: false,
        clipboardWrite: true,
        network: [],
        processSpawn: false,
        notifications: false,
        settingsStorage: false,
      };
    }

    it('maps discovered plugins including their permissions and diagnostics', async () => {
      requestListPlugins.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: [
          {
            id: 'example.copy-markdown',
            name: 'Copy Markdown',
            version: '1.0.0',
            description: 'Copies a markdown link',
            enabled: true,
            diagnostic: null,
            columns: [],
            permissions: fixturePermissions(),
          },
        ],
      });
      const client = new HttpFileManagerClient();

      const plugins = await client.listPlugins();

      expect(plugins).toEqual([
        {
          id: 'example.copy-markdown',
          name: 'Copy Markdown',
          version: '1.0.0',
          description: 'Copies a markdown link',
          enabled: true,
          columns: [],
          permissions: fixturePermissions(),
        },
      ]);
    });

    it('enables and disables a plugin through the matching generated endpoint', async () => {
      requestEnablePlugin.mockResolvedValue({ status: 204, headers: new Headers() });
      requestDisablePlugin.mockResolvedValue({ status: 204, headers: new Headers() });
      const controller = new AbortController();
      const client = new HttpFileManagerClient();

      await client.setPluginEnabled('example.copy-markdown', true, controller.signal);
      await client.setPluginEnabled('example.copy-markdown', false, controller.signal);

      const options = expect.objectContaining({ signal: controller.signal });
      expect(requestEnablePlugin).toHaveBeenCalledWith('example.copy-markdown', options);
      expect(requestDisablePlugin).toHaveBeenCalledWith('example.copy-markdown', options);
    });

    it('fetches a plugin bounded diagnostic log', async () => {
      requestGetPluginLogs.mockResolvedValue({
        status: 200,
        headers: new Headers(),
        data: [{ message: 'plugin execution timed out' }],
      });
      const client = new HttpFileManagerClient();

      await expect(client.getPluginLogs('example.copy-markdown')).resolves.toEqual([
        { message: 'plugin execution timed out' },
      ]);
      expect(requestGetPluginLogs).toHaveBeenCalledWith('example.copy-markdown', undefined);
    });
  });
});
