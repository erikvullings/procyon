import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const onDragDropEvent = vi.fn();
const openUrl = vi.fn();

class MockChannel<T> {
  constructor(public onmessage: (message: T) => void) {}
}

vi.mock('@tauri-apps/api/core', () => ({
  Channel: MockChannel,
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onDragDropEvent }),
}));
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (...args: unknown[]) => openUrl(...args),
}));

const { TauriFileManagerClient } = await import('./tauri-file-manager-client');

function fixtureCapabilities() {
  return {
    clipboard: true,
    extendedAttributes: false,
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
    runtime: 'tauri',
    serverAdministration: false,
    systemTrash: true,
  };
}

afterEach(() => {
  invoke.mockReset();
  onDragDropEvent.mockReset();
  openUrl.mockReset();
});

describe('TauriFileManagerClient', () => {
  describe('operation transport', () => {
    it('maps the Tauri operation discriminator to the frontend kind', async () => {
      invoke.mockResolvedValue([
        {
          id: 'operation-1',
          type: 'trash',
          state: 'completed',
          sources: [
            {
              id: 'entry-1',
              location: { providerId: 'local', uri: 'file:///Documents/report.pdf' },
            },
          ],
          destination: null,
          progress: { completedItems: 1, completedBytes: 0 },
          conflictPolicy: 'ask',
          createdAt: '2026-08-31T15:00:00Z',
          startedAt: '2026-08-31T15:00:01Z',
          completedAt: '2026-08-31T15:00:02Z',
          queuePosition: null,
          resultSummary: 'Moved 1 item to Trash.',
          errors: [],
          undo: { available: true, reason: null, operationId: null },
          undoOf: null,
        },
      ]);

      const [operation] = await new TauriFileManagerClient().listOperations();

      expect(operation).toMatchObject({
        kind: 'trash',
        completedAt: '2026-08-31T15:00:02Z',
        result: { message: 'Moved 1 item to Trash.' },
        undo: { available: true },
      });
      expect(operation).not.toHaveProperty('type');
    });
  });

  describe('settings transport', () => {
    it('normalizes optional saved-search collections omitted from Tauri JSON', async () => {
      invoke.mockResolvedValue({
        multiRenamePresets: [],
        savedSearches: [
          {
            id: 'search-1',
            name: 'Documents',
            pinned: true,
            query: {
              schemaVersion: 1,
              scope: { locations: [], recurse: true, showHidden: false },
            },
          },
        ],
        favouriteLocations: [],
        recentLocationsByWorkspace: {},
      });

      const settings = await new TauriFileManagerClient().getSettings();

      expect(settings.savedSearches[0]?.query).toMatchObject({
        entryKinds: [],
        mimeTypes: [],
        gitStatuses: [],
        tags: [],
        metadata: {},
      });
      expect(invoke).toHaveBeenCalledWith('get_settings');
    });
  });

  describe('OneDrive authorization', () => {
    it('begins through IPC and opens Microsoft in the system browser', async () => {
      const response = {
        attemptId: 'attempt-1',
        authorizationUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
      };
      invoke.mockResolvedValue(response);
      openUrl.mockResolvedValue(undefined);
      const client = new TauriFileManagerClient();

      await expect(client.beginOneDriveAuthorization('connection-1')).resolves.toEqual(response);
      expect(invoke).toHaveBeenCalledWith('begin_onedrive_authorization', {
        connectionId: 'connection-1',
      });
      expect(openUrl).toHaveBeenCalledWith(response.authorizationUrl);
    });

    it('polls and cancels through matching IPC commands', async () => {
      const attempt = { id: 'attempt-1', status: { state: 'pending' as const } };
      invoke
        .mockResolvedValueOnce(attempt)
        .mockResolvedValueOnce({ ...attempt, status: { state: 'cancelled' as const } });
      const client = new TauriFileManagerClient();

      await expect(client.getOneDriveAuthorizationAttempt('attempt-1')).resolves.toEqual(attempt);
      await expect(client.cancelOneDriveAuthorization('attempt-1')).resolves.toEqual({
        ...attempt,
        status: { state: 'cancelled' },
      });
      expect(invoke).toHaveBeenNthCalledWith(1, 'get_onedrive_authorization_attempt', {
        attemptId: 'attempt-1',
      });
      expect(invoke).toHaveBeenNthCalledWith(2, 'cancel_onedrive_authorization', {
        attemptId: 'attempt-1',
      });
    });

    it('rejects an authorization URL outside the expected Microsoft endpoint', async () => {
      invoke.mockResolvedValue({
        attemptId: 'attempt-1',
        authorizationUrl: 'https://example.test/phishing',
      });

      await expect(
        new TauriFileManagerClient().beginOneDriveAuthorization('connection-1'),
      ).rejects.toThrow('Microsoft authorization URL');
      expect(openUrl).not.toHaveBeenCalled();
    });
  });

  describe('startNativeDrag', () => {
    it('starts a native file drag with the selected locations', async () => {
      invoke.mockResolvedValue(undefined);
      const client = new TauriFileManagerClient();
      const locations = [
        { providerId: 'local', uri: 'file:///Users/example/report.pdf' },
        { providerId: 'local', uri: 'file:///Users/example/photos' },
      ] as const;

      await client.startNativeDrag(locations);

      expect(invoke).toHaveBeenCalledWith('start_native_drag', { locations });
    });

    describe('showPlatformContextMenu', () => {
      it('opens the native Services or Send To submenu for the selected locations', async () => {
        invoke.mockResolvedValue(undefined);
        const client = new TauriFileManagerClient();
        const locations = [
          { providerId: 'local', uri: 'file:///Users/example/report.pdf' },
          { providerId: 'local', uri: 'file:///Users/example/photos' },
        ] as const;

        await client.showPlatformContextMenu(locations);

        expect(invoke).toHaveBeenCalledWith('show_platform_context_menu', { locations });
      });
    });
  });

  describe('subscribeNativeFileDrops', () => {
    it('converts dropped OS paths to locations before notifying the app', async () => {
      const unlisten = vi.fn();
      let handler: ((event: { payload: object }) => Promise<void>) | undefined;
      onDragDropEvent.mockImplementation((candidate) => {
        handler = candidate;
        return Promise.resolve(unlisten);
      });
      invoke.mockResolvedValue([{ providerId: 'local', uri: 'file:///Users/example/report.pdf' }]);
      const listener = vi.fn();

      await new TauriFileManagerClient().subscribeNativeFileDrops(listener);
      await handler?.({
        payload: {
          type: 'drop',
          paths: ['/Users/example/report.pdf'],
          position: { x: 240, y: 120 },
        },
      });

      expect(invoke).toHaveBeenCalledWith('native_drag_locations', {
        paths: ['/Users/example/report.pdf'],
      });
      expect(listener).toHaveBeenCalledWith({
        locations: [{ providerId: 'local', uri: 'file:///Users/example/report.pdf' }],
        position: { x: 240, y: 120 },
      });
    });
  });

  describe('getFileIcon', () => {
    it('converts the Tauri byte array and silently falls back on errors', async () => {
      invoke.mockResolvedValueOnce([0x89, 0x50, 0x4e, 0x47]);
      const client = new TauriFileManagerClient();

      await expect(client.getFileIcon('file:///report.pdf')).resolves.toEqual(
        new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
      );
      expect(invoke).toHaveBeenCalledWith('get_file_icon', { uri: 'file:///report.pdf' });

      invoke.mockRejectedValueOnce(new Error('unsupported'));
      await expect(client.getFileIcon('file:///report.pdf')).resolves.toBeUndefined();
    });
  });

  describe('getRuntimeCapabilities', () => {
    it('invokes the get_runtime_capabilities command and returns its result', async () => {
      const fixture = fixtureCapabilities();
      invoke.mockResolvedValue(fixture);
      const client = new TauriFileManagerClient();

      const result = await client.getRuntimeCapabilities();

      expect(result).toEqual(fixture);
      expect(invoke).toHaveBeenCalledWith('get_runtime_capabilities');
    });

    it('propagates a command rejection without wrapping it', async () => {
      const commandError = new Error('boom');
      invoke.mockRejectedValue(commandError);
      const client = new TauriFileManagerClient();

      await expect(client.getRuntimeCapabilities()).rejects.toBe(commandError);
    });
  });

  describe('cancelDiskUsage', () => {
    it('invokes the matching cancellation command', async () => {
      invoke.mockResolvedValue(undefined);
      const client = new TauriFileManagerClient();

      await client.cancelDiskUsage('scan-1');

      expect(invoke).toHaveBeenCalledWith('cancel_disk_usage', { scanId: 'scan-1' });
    });
  });

  describe('getSystemLocations', () => {
    it('invokes the matching Tauri command', async () => {
      const locations = [
        {
          name: 'Example Drive',
          kind: 'cloud',
          location: { providerId: 'local', uri: 'file:///Example' },
        },
      ] as const;
      invoke.mockResolvedValue(locations);

      await expect(new TauriFileManagerClient().getSystemLocations()).resolves.toEqual(locations);
      expect(invoke).toHaveBeenCalledWith('get_system_locations');
    });
  });

  describe('getVolumes', () => {
    it('invokes the matching Tauri command', async () => {
      const volumes = [
        { name: 'Macintosh HD', location: { providerId: 'local', uri: 'file:///' } },
      ] as const;
      invoke.mockResolvedValue(volumes);

      await expect(new TauriFileManagerClient().getVolumes()).resolves.toEqual(volumes);
      expect(invoke).toHaveBeenCalledWith('get_volumes');
    });
  });

  describe('directory methods', () => {
    it('invokes navigate_pane with the request wrapper expected by Tauri', async () => {
      const snapshot = {
        paneId: 'left',
        requestId: 'request-1',
        revision: 1,
        location: { providerId: 'local', uri: 'file:///' },
        entries: [],
        hasMore: false,
        loadingState: { type: 'loaded' },
      };
      invoke.mockResolvedValue(snapshot);
      const client = new TauriFileManagerClient();
      const request = {
        workspaceId: 'workspace-1',
        paneId: 'left',
        requestId: 'request-1',
        location: { providerId: 'local', uri: 'file:///' },
      };

      await expect(client.navigatePane(request)).resolves.toEqual(snapshot);
      expect(invoke).toHaveBeenCalledWith('navigate_pane', { request });
    });
  });

  describe('discoverApplicationUninstallCandidates', () => {
    it('invokes discover_application_uninstall_candidates with the request wrapper expected by Tauri', async () => {
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
      invoke.mockResolvedValue(result);
      const client = new TauriFileManagerClient();
      const request = { location: { providerId: 'local', uri: 'file:///Applications/Widget.app' } };

      await expect(client.discoverApplicationUninstallCandidates(request)).resolves.toEqual(result);
      expect(invoke).toHaveBeenCalledWith('discover_application_uninstall_candidates', {
        request,
      });
    });
  });

  describe('removeApplicationDockIcon', () => {
    it('invokes remove_application_dock_icon with the request wrapper expected by Tauri', async () => {
      invoke.mockResolvedValue({ removed: true });
      const client = new TauriFileManagerClient();
      const request = { location: { providerId: 'local', uri: 'file:///Applications/Widget.app' } };

      await expect(client.removeApplicationDockIcon(request)).resolves.toEqual({ removed: true });
      expect(invoke).toHaveBeenCalledWith('remove_application_dock_icon', { request });
    });
  });

  describe('operation methods', () => {
    it('invokes semantic operation commands without enumerating source files', async () => {
      const request = {
        type: 'copy',
        sources: [{ providerId: 'local', uri: 'file:///Documents' }],
        destination: { providerId: 'local', uri: 'file:///Archive' },
        conflictPolicy: 'ask',
      } as const;
      const dto = {
        id: 'operation-1',
        type: 'copy',
        state: 'queued',
        sources: request.sources,
        destination: request.destination,
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: 'ask',
        createdAt: '2026-07-31T12:00:00Z',
        errors: [],
        undo: { available: false, reason: 'Operation has not completed.', operationId: null },
      };
      const operation = {
        id: dto.id,
        kind: dto.type,
        state: dto.state,
        sources: dto.sources,
        destination: dto.destination,
        progress: dto.progress,
        conflictPolicy: dto.conflictPolicy,
        createdAt: dto.createdAt,
        undo: { available: false, reason: 'Operation has not completed.' },
      };
      invoke.mockResolvedValue(dto);
      const client = new TauriFileManagerClient();

      await expect(client.startOperation(request)).resolves.toEqual(operation);
      expect(invoke).toHaveBeenCalledWith('start_operation', { request });
      await client.pauseOperation(operation.id);
      expect(invoke).toHaveBeenCalledWith('pause_operation', { operationId: operation.id });
      await client.resumeOperation(operation.id);
      expect(invoke).toHaveBeenCalledWith('resume_operation', { operationId: operation.id });
      await client.cancelOperation(operation.id);
      expect(invoke).toHaveBeenCalledWith('cancel_operation', { operationId: operation.id });
      await expect(client.undoOperation(operation.id)).resolves.toEqual(operation);
      expect(invoke).toHaveBeenCalledWith('undo_operation', { operationId: operation.id });
    });
  });

  describe('search methods', () => {
    it('invokes start_search and returns the searchId/location', async () => {
      const request = {
        query: 'report',
        roots: [{ providerId: 'local', uri: 'file:///Documents' }],
        workspaceId: 'workspace-1',
      };
      const result = {
        searchId: 'search-1',
        location: { providerId: 'local', uri: 'search://local/search-1' },
        limitations: [],
        executionMode: 'mixed',
      };
      invoke.mockResolvedValue(result);
      const client = new TauriFileManagerClient();

      await expect(client.startSearch(request)).resolves.toEqual(result);
      expect(invoke).toHaveBeenCalledWith('start_search', { request });
    });

    it('invokes cancel_search with the searchId', async () => {
      invoke.mockResolvedValue(undefined);
      const client = new TauriFileManagerClient();

      await client.cancelSearch('search-1');

      expect(invoke).toHaveBeenCalledWith('cancel_search', { searchId: 'search-1' });
    });
  });

  describe('comparison methods', () => {
    it('invokes start_comparison and returns the comparisonId', async () => {
      const request = {
        workspaceId: 'workspace-1',
        left: { providerId: 'local', uri: 'file:///left' },
        right: { providerId: 'local', uri: 'file:///right' },
        criteria: 'sizeAndTimestamp' as const,
      };
      const result = { comparisonId: 'comparison-1' };
      invoke.mockResolvedValue(result);
      const client = new TauriFileManagerClient();

      await expect(client.startComparison(request)).resolves.toEqual(result);
      expect(invoke).toHaveBeenCalledWith('start_comparison', { request });
    });

    it('invokes get_comparison with paging and filter options', async () => {
      const page = {
        comparisonId: 'comparison-1',
        left: { providerId: 'local', uri: 'file:///left' },
        right: { providerId: 'local', uri: 'file:///right' },
        criteria: 'nameOnly' as const,
        offset: 5,
        limit: 50,
        total: 1,
        entries: [],
        isComplete: true,
        warningsCount: 0,
      };
      invoke.mockResolvedValue(page);
      const client = new TauriFileManagerClient();

      await expect(
        client.getComparison('comparison-1', { offset: 5, limit: 50, differencesOnly: true }),
      ).resolves.toEqual(page);
      expect(invoke).toHaveBeenCalledWith('get_comparison', {
        comparisonId: 'comparison-1',
        offset: 5,
        limit: 50,
        differencesOnly: true,
      });
    });

    it('invokes cancel_comparison with the comparisonId', async () => {
      invoke.mockResolvedValue(undefined);
      const client = new TauriFileManagerClient();

      await client.cancelComparison('comparison-1');

      expect(invoke).toHaveBeenCalledWith('cancel_comparison', { comparisonId: 'comparison-1' });
    });

    it('invokes generate_sync_plan with the mode', async () => {
      const plan = { comparisonId: 'comparison-1', items: [] };
      invoke.mockResolvedValue(plan);
      const client = new TauriFileManagerClient();

      await expect(
        client.generateSyncPlan('comparison-1', { mode: 'twoWayUpdate' }),
      ).resolves.toEqual(plan);
      expect(invoke).toHaveBeenCalledWith('generate_sync_plan', {
        comparisonId: 'comparison-1',
        request: { mode: 'twoWayUpdate' },
      });
    });

    it('invokes apply_sync_plan, omitting undefined sides from the wire request', async () => {
      const result = { operationIds: ['operation-1'] };
      invoke.mockResolvedValue(result);
      const client = new TauriFileManagerClient();

      await expect(
        client.applySyncPlan('comparison-1', {
          items: [{ relativePath: 'a.txt', status: 'onlyLeft', action: 'copyLeftToRight' }],
        }),
      ).resolves.toEqual(result);
      expect(invoke).toHaveBeenCalledWith('apply_sync_plan', {
        comparisonId: 'comparison-1',
        request: {
          items: [{ relativePath: 'a.txt', status: 'onlyLeft', action: 'copyLeftToRight' }],
        },
      });
    });
  });

  describe('file range and content search methods', () => {
    it('invokes read_file_range and returns the chunk', async () => {
      const request = {
        location: { providerId: 'local', uri: 'file:///report.txt' },
        offset: 0,
        length: 3,
      };
      const chunk = { data: [1, 2, 3], offset: 0, length: 3, eof: false, probablyBinary: false };
      invoke.mockResolvedValue(chunk);
      const client = new TauriFileManagerClient();

      await expect(client.readFileRange(request)).resolves.toEqual(chunk);
      expect(invoke).toHaveBeenCalledWith('read_file_range', { request });
    });

    it('uses the shared DOCX session DTOs for open, resource read, and close', async () => {
      const client = new TauriFileManagerClient();
      const location = { providerId: 'local', uri: 'file:///report.docx' };
      const preview = {
        sessionId: 'docx-session',
        sourceRevision: 'r1',
        sourceBytes: 1024,
        html: '<p>Report</p>',
        resources: [],
        omittedFeatures: ['exact pagination'],
      };
      invoke
        .mockResolvedValueOnce(preview)
        .mockResolvedValueOnce({ data: [137, 80], mediaType: 'image/png' })
        .mockResolvedValueOnce(undefined);

      await expect(client.openDocxPreview({ location })).resolves.toEqual(preview);
      await expect(
        client.readDocxPreviewResource({
          sessionId: 'docx-session',
          resourceId: 'image-1',
        }),
      ).resolves.toEqual({ data: [137, 80], mediaType: 'image/png' });
      await expect(client.closeDocxPreview({ sessionId: 'docx-session' })).resolves.toBeUndefined();
      expect(invoke).toHaveBeenNthCalledWith(1, 'open_docx_preview', {
        request: { location },
      });
      expect(invoke).toHaveBeenNthCalledWith(2, 'read_docx_preview_resource', {
        request: { sessionId: 'docx-session', resourceId: 'image-1' },
      });
      expect(invoke).toHaveBeenNthCalledWith(3, 'close_docx_preview', {
        request: { sessionId: 'docx-session' },
      });
    });

    it('invokes search_in_file and returns the matches', async () => {
      const request = {
        location: { providerId: 'local', uri: 'file:///report.txt' },
        query: 'error',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
      };
      const result = { matches: [{ lineNumber: 1, offset: 0, length: 5 }], truncated: false };
      invoke.mockResolvedValue(result);
      const client = new TauriFileManagerClient();

      await expect(client.searchInFile(request)).resolves.toEqual(result);
      expect(invoke).toHaveBeenCalledWith('search_in_file', { request });
    });

    it('uses the same structured-view DTOs for open, rows, JSON, status, update, search, and close', async () => {
      const client = new TauriFileManagerClient();
      const location = { providerId: 'local', uri: 'file:///report.csv' };
      const openRequest = { location, format: 'csv' as const, headerMode: 'auto' as const };
      const view = {
        sessionId: 'session-1',
        kind: 'table',
        sourceRevision: 'r1',
        sourceBytes: 10,
        randomAccess: true,
        delimiter: ',',
        headerMode: 'auto',
        headers: ['name'],
        rows: [],
        indexedBytes: 10,
        indexedRows: 0,
        totalRows: 0,
        indexingComplete: true,
      };
      invoke.mockResolvedValueOnce(view);
      await expect(client.openStructuredView(openRequest)).resolves.toEqual(view);
      expect(invoke).toHaveBeenLastCalledWith('open_structured_view', { request: openRequest });

      invoke.mockResolvedValueOnce({
        indexedBytes: 10,
        indexedRows: 0,
        totalRows: 0,
        indexingComplete: true,
      });
      await client.getStructuredViewStatus({ sessionId: 'session-1' });
      expect(invoke).toHaveBeenLastCalledWith('structured_view_status', {
        request: { sessionId: 'session-1' },
      });

      invoke.mockResolvedValueOnce(view);
      await client.updateStructuredView({
        sessionId: 'session-1',
        delimiter: ';',
        headerMode: 'none',
      });
      expect(invoke).toHaveBeenLastCalledWith('update_structured_view', {
        request: { sessionId: 'session-1', delimiter: ';', headerMode: 'none' },
      });

      invoke.mockResolvedValueOnce({
        rows: [],
        indexedRows: 0,
        totalRows: 0,
        indexingComplete: true,
      });
      await client.readStructuredRows({ sessionId: 'session-1', startRow: 0, count: 100 });
      expect(invoke).toHaveBeenLastCalledWith('read_structured_rows', {
        request: { sessionId: 'session-1', startRow: 0, count: 100 },
      });

      invoke.mockResolvedValueOnce({ data: [], offset: 0, eof: true, tokens: [] });
      await client.readStructuredJsonWindow({ sessionId: 'session-1', offset: 0, length: 65_536 });
      expect(invoke).toHaveBeenLastCalledWith('read_structured_json_window', {
        request: { sessionId: 'session-1', offset: 0, length: 65_536 },
      });

      invoke.mockResolvedValueOnce({ matches: [], nextCursor: null, indexingComplete: true });
      await client.searchStructuredRows({
        sessionId: 'session-1',
        query: 'Ada',
        cursor: 0,
        limit: 20,
      });
      expect(invoke).toHaveBeenLastCalledWith('search_structured_rows', {
        request: { sessionId: 'session-1', query: 'Ada', cursor: 0, limit: 20 },
      });

      invoke.mockResolvedValueOnce(undefined);
      await client.closeStructuredView({ sessionId: 'session-1' });
      expect(invoke).toHaveBeenLastCalledWith('close_structured_view', {
        request: { sessionId: 'session-1' },
      });
    });
  });

  describe('workspace methods', () => {
    it('invokes get_workspace and normalizes the result', async () => {
      invoke.mockResolvedValue({
        id: 'workspace-1',
        name: 'Workspace',
        revision: 1,
        layout: { type: 'pane', paneId: 'pane-1' },
        panes: [],
        activePaneId: 'pane-1',
        operationCentre: { visible: false, height: 180 },
      });
      const client = new TauriFileManagerClient();

      await expect(client.getWorkspace('workspace-1')).resolves.toEqual(
        expect.objectContaining({ id: 'workspace-1', panesById: {} }),
      );
      expect(invoke).toHaveBeenCalledWith('get_workspace', { workspaceId: 'workspace-1' });
    });
  });

  describe('subscribe', () => {
    it('connects the Tauri event stream and forwards dispatched events to the listener', async () => {
      invoke.mockResolvedValue('subscription-1');
      const client = new TauriFileManagerClient();
      const listener = vi.fn();

      const unsubscribe = await client.subscribe(listener);

      expect(typeof unsubscribe).toBe('function');
      expect(invoke).toHaveBeenCalledWith('subscribe_events', {
        onEvent: expect.any(MockChannel),
      });
    });
  });
});
