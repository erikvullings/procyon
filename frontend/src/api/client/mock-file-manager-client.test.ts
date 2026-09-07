import { describe, expect, it, vi } from 'vitest';

import type { BackendEvent, DirectoryDelta } from '../../models';
import { MockClientError, MockFileManagerClient } from './mock-file-manager-client';

const ROOT_REQUEST = {
  workspaceId: 'workspace-1',
  paneId: 'left',
  requestId: 'request-1',
  location: { providerId: 'file', uri: 'mock:///' },
} as const;

describe('MockFileManagerClient directories', () => {
  it('mirrors bounded workbook sheets and in-session sheet selection', async () => {
    const client = new MockFileManagerClient();
    const opened = await client.openStructuredView({
      location: { providerId: 'file', uri: 'mock:///budget.xlsx' },
      format: 'excel',
      headerMode: 'none',
    });

    expect(opened).toMatchObject({
      kind: 'table',
      selectedSheet: 'Summary',
      sheets: [{ name: 'Summary' }, { name: 'Details' }],
    });
    const selected = await client.updateStructuredView({
      sessionId: opened.sessionId,
      selectedSheet: 'Details',
    });
    expect(selected).toMatchObject({
      selectedSheet: 'Details',
      rows: [
        { index: 0, cells: ['Details'] },
        { index: 1, cells: [] },
        { index: 2, cells: ['Sparse row'] },
      ],
    });
  });

  it('shows the extension column by default', async () => {
    const settings = await new MockFileManagerClient().getSettings();

    expect(settings.defaultColumns).toContain('core.extension');
  });

  it('lists deterministic nested and special-case fixture entries', async () => {
    const client = new MockFileManagerClient();

    const root = await client.listDirectory(ROOT_REQUEST);
    const nested = await client.listDirectory({
      ...ROOT_REQUEST,
      requestId: 'request-2',
      location: { providerId: 'file', uri: 'mock:///Documents' },
    });

    expect(root.entries.map(({ name, kind, hidden }) => ({ name, kind, hidden }))).toEqual([
      { name: 'Documents', kind: 'directory', hidden: false },
      { name: 'Empty', kind: 'directory', hidden: false },
      { name: 'Unreadable', kind: 'directory', hidden: false },
      { name: 'Applications', kind: 'directory', hidden: false },
      { name: '.env', kind: 'file', hidden: true },
      { name: '日本語.txt', kind: 'file', hidden: false },
      { name: 'documents-link', kind: 'symlink', hidden: false },
    ]);
    // Deliberately out of display order (a file before a directory) - the mock client passes
    // fixture entries through unsorted, matching a real backend, so sorting/cursor-placement bugs
    // that only show up with an unsorted listing (e.g. app-shell.test.ts's cursor-on-navigate
    // test) aren't masked by the fixture happening to already be alphabetical.
    expect(nested.entries.map((entry) => entry.name)).toEqual(['report.pdf', 'Projects']);
  });

  describe('MockFileManagerClient OneDrive authorization', () => {
    it('authorizes a saved OneDrive connection without exposing token material', async () => {
      const client = new MockFileManagerClient();
      const connection = await client.createConnection({
        name: 'Work OneDrive',
        kind: 'oneDrive',
        configuration: { kind: 'oneDrive', accountHint: 'erik@example.test' },
        secret: null,
      });

      const begun = await client.beginOneDriveAuthorization(connection.id);
      const completed = await client.getOneDriveAuthorizationAttempt(begun.attemptId);

      expect(begun.authorizationUrl).toMatch(/^https:\/\/login\.microsoftonline\.com\//);
      expect(completed.status).toMatchObject({
        state: 'succeeded',
        connection: {
          id: connection.id,
          hasCredential: true,
          rootLocation: `onedrive://${connection.id}/`,
          configuration: {
            kind: 'oneDrive',
            email: 'erik@example.test',
            driveType: 'business',
          },
        },
      });
      expect(JSON.stringify(completed)).not.toMatch(/accessToken|refreshToken/i);
    });

    it('cancels a pending OneDrive authorization attempt', async () => {
      const client = new MockFileManagerClient();
      const connection = await client.createConnection({
        name: 'Personal OneDrive',
        kind: 'oneDrive',
        configuration: { kind: 'oneDrive', accountHint: null },
        secret: null,
      });

      const begun = await client.beginOneDriveAuthorization(connection.id);

      await expect(client.cancelOneDriveAuthorization(begun.attemptId)).resolves.toEqual({
        id: begun.attemptId,
        status: { state: 'cancelled' },
      });
    });
  });

  it('listDirectoryChildren returns only the directory-kind fixture entries', async () => {
    const client = new MockFileManagerClient();

    const children = await client.listDirectoryChildren(
      { providerId: 'file', uri: 'mock:///' },
      false,
    );

    expect(children.map((entry) => entry.name)).toEqual([
      'Documents',
      'Empty',
      'Unreadable',
      'Applications',
    ]);
    expect(children.every((entry) => entry.kind === 'directory')).toBe(true);
  });

  it('listDirectoryChildren returns an empty list for a location with no fixture', async () => {
    const client = new MockFileManagerClient();

    const children = await client.listDirectoryChildren(
      { providerId: 'file', uri: 'mock:///Empty' },
      false,
    );

    expect(children).toEqual([]);
  });

  it('reports accurate size/file-count totals for the full directory, not just the loaded page', async () => {
    const client = new MockFileManagerClient();

    const root = await client.listDirectory(ROOT_REQUEST);

    // 4 directories (Documents, Empty, Unreadable, Applications) + 2 files (.env: 42 bytes,
    // 日本語.txt: 128 bytes) + 1 symlink (documents-link, no reported size) = 7 entries,
    // 3 non-directory.
    expect(root.totalKnownEntries).toBe(7);
    expect(root.totalKnownFileCount).toBe(3);
    expect(root.totalKnownSize).toBe(42 + 128);
  });

  it('pages a million-entry directory without returning every entry', async () => {
    const client = new MockFileManagerClient({ pageSize: 25, seed: 99 });

    const first = await client.listDirectory({
      ...ROOT_REQUEST,
      location: { providerId: 'file', uri: 'mock:///large/1000000' },
    });
    const nextToken = first.continuationToken;
    expect(nextToken).toBe('25');
    if (nextToken === undefined) {
      throw new Error('Expected the first large-directory page to have a continuation token');
    }
    const second = await client.listDirectory({
      ...ROOT_REQUEST,
      continuationToken: nextToken,
      location: { providerId: 'file', uri: 'mock:///large/1000000' },
    });

    expect(first.entries).toHaveLength(25);
    expect(first.totalKnownEntries).toBe(1_000_000);
    expect(first.hasMore).toBe(true);
    expect(second.entries[0]?.id).not.toBe(first.entries[0]?.id);
  });

  it('reports the full generated directory total size/file count, cached across pages', async () => {
    const client = new MockFileManagerClient({ pageSize: 25, seed: 99 });

    const first = await client.listDirectory({
      ...ROOT_REQUEST,
      location: { providerId: 'file', uri: 'mock:///large/1000000' },
    });
    const nextToken = first.continuationToken;
    if (nextToken === undefined) {
      throw new Error('Expected the first large-directory page to have a continuation token');
    }
    const second = await client.listDirectory({
      ...ROOT_REQUEST,
      continuationToken: nextToken,
      location: { providerId: 'file', uri: 'mock:///large/1000000' },
    });

    // Every generated entry is a file, so the file count always equals the entry total.
    expect(first.totalKnownFileCount).toBe(1_000_000);
    expect(first.totalKnownSize).toBeGreaterThan(0);
    // The aggregate is a pure function of (size, seed); it must be identical across pages/requests.
    expect(second.totalKnownFileCount).toBe(first.totalKnownFileCount);
    expect(second.totalKnownSize).toBe(first.totalKnownSize);
  });

  it('returns error and loading snapshots for configured directory states', async () => {
    const client = new MockFileManagerClient({
      loadingLocations: ['mock:///Documents'],
    });

    const unreadable = await client.listDirectory({
      ...ROOT_REQUEST,
      location: { providerId: 'file', uri: 'mock:///Unreadable' },
    });
    const loading = await client.navigatePane({
      ...ROOT_REQUEST,
      location: { providerId: 'file', uri: 'mock:///Documents' },
    });

    expect(unreadable.loadingState).toEqual({
      type: 'error',
      message: 'Directory is not readable',
    });
    expect(loading.loadingState).toEqual({ type: 'loading' });
  });
});

describe('MockFileManagerClient API', () => {
  it('returns deterministic native icon bytes only for configured extensions', async () => {
    const client = new MockFileManagerClient({ nativeIconExtensions: ['pdf'] });

    expect((await client.getRuntimeCapabilities()).nativeFileIcons).toBe(true);
    await expect(client.getFileIcon('mock:///report.PDF')).resolves.toEqual(expect.any(Uint8Array));
    await expect(client.getFileIcon('mock:///notes.txt')).resolves.toBeUndefined();
  });

  it('provides deterministic capabilities, workspace, metadata, actions, and plugins', async () => {
    const client = new MockFileManagerClient();

    const capabilities = await client.getRuntimeCapabilities();
    const workspace = await client.getWorkspace('mock-workspace');
    const metadata = await client.getEntryMetadata({
      entryId: 'mock:///日本語.txt',
      location: { providerId: 'file', uri: 'mock:///%E6%97%A5%E6%9C%AC%E8%AA%9E.txt' },
    });
    const actions = await client.listActions();
    const plugins = await client.listPlugins();
    const actionResult = await client.invokeAction({ actionId: 'core.refresh', context: {} });

    expect(capabilities.runtime).toBe('mock');
    expect(workspace.id).toBe('mock-workspace');
    expect(metadata.entryId).toBe('mock:///日本語.txt');
    expect(actions.map((action) => action.id)).toEqual([
      'core.refresh',
      'core.rename',
      'core.copy',
      'core.pack',
      'core.moveToArchive',
      'core.extract',
      'core.move',
      'core.createDirectory',
      'core.paste',
      'core.trash',
      'core.delete',
      'core.palette',
      'core.focusLocation',
      'core.quickFilter',
      'core.findFiles',
      'core.newTab',
      'core.closeTab',
      'core.nextTab',
      'core.previousTab',
      'core.reopenClosedTab',
      'core.open',
      'core.view',
      'core.calculateFolderSize',
      'core.edit',
      'core.openWith',
      'core.quickLook',
      'core.revealInSystemFileManager',
      'core.uninstallApplication',
      'core.openTerminal',
      'core.copyName',
      'core.copyPath',
      'core.copyRelativePath',
      'core.parent',
      'core.switchPane',
      'core.moveCursorUp',
      'core.moveCursorDown',
      'core.moveCursorPageUp',
      'core.moveCursorPageDown',
      'core.moveCursorFirst',
      'core.moveCursorLast',
      'core.extendSelectionUp',
      'core.extendSelectionDown',
      'core.toggleSelection',
      'core.toggleSelectionAndAdvance',
      'core.selectAll',
      'core.clearSelection',
    ]);
    expect(plugins.map((plugin) => plugin.id)).toEqual(['mock.archive']);
    expect(actionResult).toEqual({ actionId: 'core.refresh', invoked: true });
  });

  it('tracks operation lifecycle calls in memory', async () => {
    const client = new MockFileManagerClient({ seed: 22 });
    const operation = await client.startOperation({
      type: 'copy',
      sources: [
        {
          providerId: 'file',
          uri: 'mock:///Documents/report.pdf',
        },
      ],
      destination: { providerId: 'file', uri: 'mock:///Empty' },
      conflictPolicy: 'ask',
    });

    await client.resolveConflict({
      operationId: operation.id,
      resolution: 'skip',
      applyToAllSimilar: false,
    });
    await client.cancelOperation(operation.id);

    expect(client.getOperation(operation.id)).toMatchObject({
      state: 'cancelled',
      conflictPolicy: 'skip',
    });
  });

  it('implements workspace lifecycle and semantic commands in memory', async () => {
    const client = new MockFileManagerClient();
    const created = await client.createWorkspace({ name: 'Projects' });
    const renamed = await client.renameWorkspace(created.id, 'Development', created.revision);
    const changed = await client.dispatchWorkspaceCommand({
      type: 'addTab',
      workspaceId: created.id,
      expectedRevision: renamed.revision,
      paneId: 'left',
      location: { providerId: 'file', uri: 'mock:///Documents' },
    });

    expect((await client.listWorkspaces()).map((workspace) => workspace.name)).toEqual([
      'Development',
    ]);
    expect(changed.panesById.left?.tabOrder).toHaveLength(2);
    await client.deleteWorkspace(changed.id, changed.revision);
    expect(await client.listWorkspaces()).toEqual([]);
  });
});

describe('MockFileManagerClient semantic component lifecycle', () => {
  it('defaults to an absent, no-download state and exposes all deterministic profiles', async () => {
    const client = new MockFileManagerClient();

    const status = await client.getSemanticComponentStatus();
    const profiles = await client.listSemanticComponentProfiles();

    expect(status).toMatchObject({
      lifecycle: { state: 'absent' },
      components: [],
      diskUse: { totalBytes: 0 },
    });
    expect(profiles.map(({ profile, recommended }) => ({ profile, recommended }))).toEqual([
      { profile: 'compactMultilingual', recommended: true },
      { profile: 'compactEnglish', recommended: false },
      { profile: 'multilingualQuality', recommended: false },
    ]);
  });

  it.each([
    ['unavailable', { state: 'unavailable' }],
    ['absent', { state: 'absent' }],
    ['offered', { state: 'offered', offerId: 'mock-scenario-offer' }],
    [
      'downloadingResumable',
      { state: 'downloading', downloadedBytes: 160, totalBytes: 400, resumable: true },
    ],
    ['installedEnabled', { state: 'installedEnabled' }],
    ['paused', { state: 'paused' }],
    ['migrating', { state: 'migrating' }],
    [
      'updateFailedRolledBack',
      { state: 'updateFailedRolledBack', activeVersion: '1.0.0', failedVersion: '1.0.1' },
    ],
    ['lowDisk', { state: 'lowDisk', availableBytes: 512, requiredBytes: 1_024 }],
    ['uninstalledRetain', { state: 'uninstalled', indexDecision: 'retain' }],
    ['uninstalledDelete', { state: 'uninstalled', indexDecision: 'delete' }],
  ] as const)(
    'simulates the %s lifecycle variant',
    async (semanticLifecycle, expectedLifecycle) => {
      const client = new MockFileManagerClient({ semanticLifecycle });

      expect((await client.getSemanticComponentStatus()).lifecycle).toMatchObject(
        expectedLifecycle,
      );
    },
  );

  it('reports unavailable authority without exposing lifecycle operations', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'unavailable' });

    await expect(client.getSemanticComponentCapabilities()).resolves.toEqual({
      authority: 'unavailable',
      operations: [],
      runtimeExecutableDownload: 'unavailable',
    });
    await expect(client.listSemanticComponentProfiles()).rejects.toMatchObject({
      code: 'unavailable',
    });
  });

  it('creates a complete offer and transitions through install, pause, resume, and move', async () => {
    const client = new MockFileManagerClient();

    const offer = await client.createSemanticComponentInstallationOffer({
      profile: 'compactMultilingual',
    });

    expect(offer).toMatchObject({
      profile: 'compactMultilingual',
      resolvedModel: {
        modelId: 'mock-compact-multilingual',
        revision: 'mock-compact-multilingual-revision',
      },
      catalogRevision: 'mock-signed-catalog-revision',
      embeddingsStayLocal: true,
      dataRoot: 'mock/semantic',
      minimumFreeSpaceReserveBytes: 1_024,
    });
    expect(offer.components.map(({ kind }) => kind)).toEqual(['worker', 'runtime', 'model']);

    await client.acceptSemanticComponentInstallationOffer({ offerId: offer.offerId });
    expect(await client.getSemanticComponentStatus()).toMatchObject({
      lifecycle: { state: 'installedEnabled' },
      activeModel: {
        profile: 'compactMultilingual',
        identity: offer.resolvedModel,
      },
      components: [
        { kind: 'worker', version: '1.0.0' },
        { kind: 'runtime', version: '1.0.0' },
        { kind: 'model', version: '1.0.0' },
      ],
      diskUse: { totalBytes: 431 },
    });

    await client.pauseSemanticComponentIndexing();
    expect((await client.getSemanticComponentStatus()).lifecycle).toEqual({ state: 'paused' });
    await client.resumeSemanticComponentIndexing();
    expect((await client.getSemanticComponentStatus()).lifecycle).toEqual({
      state: 'installedEnabled',
    });
    await expect(
      client.moveSemanticComponentData({ destination: 'mock/moved-semantic' }),
    ).resolves.toEqual({
      source: 'mock/semantic',
      destination: 'mock/moved-semantic',
      verifiedBytes: 431,
      verifiedFileCount: 3,
    });
    expect((await client.getSemanticComponentStatus()).dataRoot).toBe('mock/moved-semantic');
  });

  it('plans from authoritative inventory, confirms by opaque ID, and rejects stale plans', async () => {
    const expected = {
      indexRecords: 7,
      extractedFiles: 6,
      zvecVectors: 5,
      cacheEntries: 4,
      conversationEvidence: 3,
    };
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });

    const firstPlan = await client.createSemanticComponentIndexRemovalPlan({
      enrolmentId: 'library-1',
    });
    const stalePlan = await client.createSemanticComponentIndexRemovalPlan({
      enrolmentId: 'library-1',
    });
    expect(firstPlan).toEqual({
      planId: 'mock-index-removal-1',
      enrolmentId: 'library-1',
      expected,
    });
    await expect(
      client.confirmSemanticComponentIndexRemoval({ planId: firstPlan.planId }),
    ).resolves.toEqual({
      enrolmentId: 'library-1',
      deleted: expected,
      conversationEvidenceDeleted: true,
    });
    await expect(
      client.confirmSemanticComponentIndexRemoval({ planId: stalePlan.planId }),
    ).rejects.toMatchObject({
      code: 'indexRemoval',
    });
    await expect(
      client.confirmSemanticComponentIndexRemoval({ planId: 'unknown-plan' }),
    ).rejects.toMatchObject({
      code: 'indexRemoval',
    });
  });

  it('applies the explicit uninstall retention decision', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });

    await expect(client.uninstallSemanticComponents({ indexDecision: 'retain' })).resolves.toEqual({
      indexDecision: 'retain',
      removedComponentCount: 3,
    });
    expect(await client.getSemanticComponentStatus()).toMatchObject({
      lifecycle: { state: 'uninstalled', indexDecision: 'retain' },
      components: [],
    });

    const deletingClient = new MockFileManagerClient({
      semanticLifecycle: 'installedEnabled',
    });
    await deletingClient.uninstallSemanticComponents({ indexDecision: 'delete' });
    expect(await deletingClient.getSemanticComponentStatus()).toMatchObject({
      lifecycle: { state: 'uninstalled', indexDecision: 'delete' },
      activeModel: null,
      diskUse: { totalBytes: 0 },
    });
  });

  it('plans, confirms, checkpoints, and completes a resumable model migration', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const plan = await client.planSemanticComponentModelMigration({
      profile: 'multilingualQuality',
      estimate: { documents: 10, sourceBytes: 1_000 },
    });

    expect(plan).toMatchObject({
      target: { profile: 'multilingualQuality' },
      fullReindex: true,
      requiresConfirmation: true,
      resumable: true,
    });
    await expect(
      client.confirmSemanticComponentModelMigration({ migrationId: plan.migrationId }),
    ).resolves.toMatchObject({ completedDocuments: 0, resumeCursor: null });
    await expect(
      client.checkpointSemanticComponentModelMigration({
        migrationId: plan.migrationId,
        completedDocuments: 10,
        resumeCursor: 'cursor-10',
      }),
    ).resolves.toMatchObject({ completedDocuments: 10, resumeCursor: 'cursor-10' });
    await expect(
      client.completeSemanticComponentModelMigration({ migrationId: plan.migrationId }),
    ).resolves.toEqual(plan.target);
    expect(await client.getSemanticComponentStatus()).toMatchObject({
      lifecycle: { state: 'installedEnabled' },
      activeModel: plan.target,
      migration: null,
    });
  });

  it('supports failure injection for semantic actions', async () => {
    const failure = new MockClientError('insufficientSpace', 'Not enough free space');
    const client = new MockFileManagerClient({
      failures: { acceptSemanticComponentInstallationOffer: failure },
    });

    await expect(
      client.acceptSemanticComponentInstallationOffer({ offerId: 'missing' }),
    ).rejects.toBe(failure);
  });
});

describe('MockFileManagerClient semantic library lifecycle', () => {
  it('previews, enrols, pauses, excludes, and keeps counts backend-owned', async () => {
    const client = new MockFileManagerClient();
    const workspace = await client.startWorkspace();
    const pane = workspace.panesById[workspace.activePaneId];
    const location = pane?.tabsById[pane.activeTabId]?.location;
    expect(location).toBeDefined();
    if (location === undefined) return;

    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    expect(preview).toMatchObject({
      policyRevision: 1,
      normalizedExcerptsRetainedLocally: true,
      estimate: {
        completeness: 'partial',
        estimatedFiles: 42,
        missingModelDownloadBytes: 500,
      },
    });
    const enrolled = await client.confirmSemanticEnrolment({
      confirmationId: preview.confirmationId,
      policyRevision: preview.policyRevision,
      workspaceId: workspace.id,
      location,
    });
    expect(enrolled.roots).toHaveLength(1);
    await expect(
      client.getSemanticFolderStatus({ workspaceId: workspace.id, location }),
    ).resolves.toMatchObject({ consent: 'includedHere' });

    await expect(
      client.pauseSemanticLibrary({ policyRevision: preview.policyRevision }),
    ).rejects.toMatchObject({ code: 'staleRevision' });
    const paused = await client.pauseSemanticLibrary({ policyRevision: enrolled.revision });
    expect(paused.paused).toBe(true);
    const resumed = await client.resumeSemanticLibrary({ policyRevision: paused.revision });
    expect(resumed.paused).toBe(false);

    const plan = await client.planSemanticExclusion({
      policyRevision: resumed.revision,
      workspaceId: workspace.id,
      location,
    });
    expect(plan.categories.map(({ category }) => category)).toEqual([
      'occurrences',
      'extractedContent',
      'summaries',
      'labels',
      'orphanVectors',
      'conversationEvidencePins',
    ]);
    const excluded = await client.confirmSemanticExclusion({
      confirmationId: plan.confirmationId,
      policyRevision: plan.policyRevision,
      workspaceId: workspace.id,
      location,
    });
    expect(excluded.roots[0]?.exclusions[0]?.cleanup.status).toBe('complete');
  });

  it('detaches a deleted workspace without revoking global root consent', async () => {
    const client = new MockFileManagerClient();
    const workspace = await client.startWorkspace();
    const pane = workspace.panesById[workspace.activePaneId];
    const location = pane?.tabsById[pane.activeTabId]?.location;
    expect(location).toBeDefined();
    if (location === undefined) return;
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    await client.confirmSemanticEnrolment({
      confirmationId: preview.confirmationId,
      policyRevision: preview.policyRevision,
      workspaceId: workspace.id,
      location,
    });

    await client.deleteWorkspace(workspace.id, workspace.revision);

    const status = await client.getSemanticLibraryStatus();
    expect(status.roots).toHaveLength(1);
    expect(status.roots[0]?.workspaceReferences).toEqual([]);
  });
});

describe('MockFileManagerClient controls', () => {
  it('delivers scripted directory-delta and operation-progress events on demand', async () => {
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    const unsubscribe = await client.subscribe(listener);
    const events: BackendEvent[] = [
      {
        eventId: 1,
        timestamp: '2026-01-01T00:00:00.000Z',
        payload: {
          type: 'directory.delta',
          paneId: 'left',
          delta: {
            type: 'entriesRemoved',
            revision: 2,
            entryIds: ['entry-1'],
          } satisfies DirectoryDelta,
        },
      },
      {
        eventId: 2,
        timestamp: '2026-01-01T00:00:01.000Z',
        payload: {
          type: 'operation.progress',
          operationId: 'operation-1',
          progress: { completedItems: 1, completedBytes: 512 },
        },
      },
    ];

    client.scriptEvents(events);
    expect(client.emitNextEvent()).toBe(true);
    expect(client.emitNextEvent()).toBe(true);
    expect(client.emitNextEvent()).toBe(false);
    unsubscribe();
    client.emit(events[0] as BackendEvent);

    expect(listener.mock.calls.map((call) => (call[0] as BackendEvent).eventId)).toEqual([1, 2]);
  });

  it('applies artificial latency and supports aborting during the delay', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient({ latencyMs: 500 });
    const controller = new AbortController();
    const result = client.getRuntimeCapabilities(controller.signal);
    const rejection = expect(result).rejects.toMatchObject({ name: 'AbortError' });
    controller.abort();
    await vi.runAllTimersAsync();

    await rejection;
    vi.useRealTimers();
  });

  it('injects configured failures by method', async () => {
    const failure = new MockClientError('offline', 'Mock backend is offline');
    const client = new MockFileManagerClient({
      failures: { listDirectory: failure },
    });

    await expect(client.listDirectory(ROOT_REQUEST)).rejects.toBe(failure);
  });
});

describe('MockFileManagerClient search methods', () => {
  it('recursively matches filenames by substring and streams a completed resultsBatch event', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    const result = await client.startSearch({
      query: 'report',
      roots: [{ providerId: 'file', uri: 'mock:///' }],
      workspaceId: 'workspace-1',
    });

    expect(result.searchId).toMatch(/^mock-search-/);
    expect(result.location).toEqual({
      providerId: 'local',
      uri: `search://local/${result.searchId}`,
    });

    await vi.runAllTimersAsync();
    vi.useRealTimers();

    expect(listener).toHaveBeenCalledOnce();
    const event = listener.mock.calls[0]?.[0] as BackendEvent;
    expect(event.payload).toMatchObject({
      type: 'search.resultsBatch',
      searchId: result.searchId,
      isComplete: true,
      warningsCount: 0,
    });
    expect(event.payload).toMatchObject({
      entries: [expect.objectContaining({ name: 'report.pdf' })],
    });
  });

  it('matches a glob query recursively across nested fixture directories', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    await client.startSearch({
      query: '*.md',
      roots: [{ providerId: 'file', uri: 'mock:///' }],
      workspaceId: 'workspace-1',
    });
    await vi.runAllTimersAsync();
    vi.useRealTimers();

    const event = listener.mock.calls[0]?.[0] as BackendEvent;
    expect(event.payload).toMatchObject({
      entries: [expect.objectContaining({ name: 'file-manager.md' })],
    });
  });

  it('treats comma-separated glob patterns as alternatives', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    await client.startSearch({
      query: '*.md, *.pdf, *.epub, *.docx',
      roots: [{ providerId: 'file', uri: 'mock:///' }],
      workspaceId: 'workspace-1',
    });
    await vi.runAllTimersAsync();
    vi.useRealTimers();

    const event = listener.mock.calls[0]?.[0] as BackendEvent;
    expect(event.payload).toMatchObject({
      entries: expect.arrayContaining([
        expect.objectContaining({ name: 'file-manager.md' }),
        expect.objectContaining({ name: 'report.pdf' }),
      ]),
    });
  });

  it('honours structured filename and content matching semantics', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);
    const scope = {
      locations: [{ providerId: 'file', uri: 'mock:///' }],
      recurse: true,
      showHidden: false,
    };
    const base = {
      schemaVersion: 1 as const,
      scope,
      entryKinds: ['file' as const],
      mimeTypes: [],
      gitStatuses: [],
      tags: [],
      metadata: {},
    };

    await client.startSearch({
      query: '',
      roots: scope.locations,
      workspaceId: 'workspace-1',
      structuredQuery: {
        ...base,
        name: { pattern: '*.md', mode: 'substring', caseSensitive: false },
      },
    });
    await client.startSearch({
      query: '',
      roots: scope.locations,
      workspaceId: 'workspace-1',
      structuredQuery: {
        ...base,
        name: { pattern: 'REPORT', mode: 'substring', caseSensitive: true },
      },
    });
    await client.startSearch({
      query: '',
      roots: scope.locations,
      workspaceId: 'workspace-1',
      structuredQuery: {
        ...base,
        name: { pattern: 'report', mode: 'substring', caseSensitive: false },
        content: { query: 'ERROR$', regex: true, caseSensitive: true, wholeWord: false },
      },
    });
    await client.startSearch({
      query: '',
      roots: scope.locations,
      workspaceId: 'workspace-1',
      structuredQuery: {
        ...base,
        name: { pattern: 'report', mode: 'substring', caseSensitive: false },
        content: { query: 'port', regex: false, caseSensitive: false, wholeWord: true },
      },
    });
    await vi.runAllTimersAsync();
    vi.useRealTimers();

    const events = listener.mock.calls.map((call) => call[0] as BackendEvent);
    expect(
      events.map((event) => ('entries' in event.payload ? event.payload.entries.length : -1)),
    ).toEqual([0, 0, 1, 0]);
  });

  it('never emits a resultsBatch for a search cancelled before it fires', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    const result = await client.startSearch({
      query: 'report',
      roots: [{ providerId: 'file', uri: 'mock:///' }],
      workspaceId: 'workspace-1',
    });
    await client.cancelSearch(result.searchId);
    await vi.runAllTimersAsync();
    vi.useRealTimers();

    expect(listener).not.toHaveBeenCalled();
  });

  it('rejects cancelling an unknown search id', async () => {
    const client = new MockFileManagerClient();

    await expect(client.cancelSearch('nonexistent')).rejects.toMatchObject({
      code: 'searchNotFound',
    });
  });
});

describe('MockFileManagerClient comparison methods', () => {
  it('compares a root against itself and reports every entry identical', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    const started = await client.startComparison({
      workspaceId: 'workspace-1',
      left: { providerId: 'file', uri: 'mock:///Documents' },
      right: { providerId: 'file', uri: 'mock:///Documents' },
      criteria: 'sizeAndTimestamp',
    });
    expect(started.comparisonId).toMatch(/^mock-comparison-/);

    await vi.runAllTimersAsync();
    vi.useRealTimers();

    const event = listener.mock.calls[0]?.[0] as BackendEvent;
    expect(event.payload).toMatchObject({
      type: 'comparison.resultsBatch',
      comparisonId: started.comparisonId,
      isComplete: true,
    });
    const page = await client.getComparison(started.comparisonId);
    expect(page.entries.length).toBeGreaterThan(0);
    expect(page.entries.every((entry) => entry.status === 'identical')).toBe(true);
  });

  it('reports entries missing from the right side as onlyLeft, filterable to differences only', async () => {
    const client = new MockFileManagerClient();
    const started = await client.startComparison({
      workspaceId: 'workspace-1',
      left: { providerId: 'file', uri: 'mock:///Documents' },
      right: { providerId: 'file', uri: 'mock:///Empty' },
      criteria: 'nameOnly',
    });

    const all = await client.getComparison(started.comparisonId);
    const names = all.entries.map((entry) => entry.relativePath).sort();
    expect(names).toEqual(['Projects', 'Projects/file-manager.md', 'report.pdf']);
    expect(all.entries.every((entry) => entry.status === 'onlyLeft')).toBe(true);

    const filtered = await client.getComparison(started.comparisonId, {
      differencesOnly: true,
    });
    expect(filtered.total).toBe(all.entries.length);
  });

  it('never emits a resultsBatch for a comparison cancelled before it fires', async () => {
    vi.useFakeTimers();
    const client = new MockFileManagerClient();
    const listener = vi.fn();
    await client.subscribe(listener);

    const started = await client.startComparison({
      workspaceId: 'workspace-1',
      left: { providerId: 'file', uri: 'mock:///Documents' },
      right: { providerId: 'file', uri: 'mock:///Empty' },
      criteria: 'nameOnly',
    });
    await client.cancelComparison(started.comparisonId);
    await vi.runAllTimersAsync();
    vi.useRealTimers();

    expect(listener).not.toHaveBeenCalled();
  });

  it('rejects operations on an unknown comparison id', async () => {
    const client = new MockFileManagerClient();

    await expect(client.getComparison('nonexistent')).rejects.toMatchObject({
      code: 'comparisonNotFound',
    });
    await expect(client.cancelComparison('nonexistent')).rejects.toMatchObject({
      code: 'comparisonNotFound',
    });
    await expect(
      client.generateSyncPlan('nonexistent', { mode: 'mirrorLeftToRight' }),
    ).rejects.toMatchObject({ code: 'comparisonNotFound' });
  });

  it('generates a mirror-left-to-right sync plan and applies it as real mock operations', async () => {
    const client = new MockFileManagerClient();
    const started = await client.startComparison({
      workspaceId: 'workspace-1',
      left: { providerId: 'file', uri: 'mock:///Documents' },
      right: { providerId: 'file', uri: 'mock:///Empty' },
      criteria: 'nameOnly',
    });

    const plan = await client.generateSyncPlan(started.comparisonId, {
      mode: 'mirrorLeftToRight',
    });
    expect(plan.items.length).toBeGreaterThan(0);
    expect(plan.items.every((item) => item.action === 'copyLeftToRight')).toBe(true);

    // Force one row to `skip` to verify it starts no operation.
    const items = plan.items.map((item, index) =>
      index === 0 ? { ...item, action: 'skip' as const } : item,
    );
    const applied = await client.applySyncPlan(started.comparisonId, { items });
    expect(applied.operationIds).toHaveLength(items.length - 1);

    const operations = await client.listOperations();
    for (const operationId of applied.operationIds) {
      const operation = operations.find((candidate) => candidate.id === operationId);
      expect(operation).toMatchObject({ kind: 'copy', state: 'completed' });
    }
  });
});

describe('MockFileManagerClient file range and content search methods', () => {
  const LOCATION = { providerId: 'file', uri: 'mock:///report.txt' } as const;

  it('provides and cleans up the same bounded DOCX session contract as other hosts', async () => {
    const client = new MockFileManagerClient();
    const opened = await client.openDocxPreview({
      location: { ...LOCATION, uri: 'mock:///report.docx' },
    });

    const resource = opened.resources[0];

    expect(opened.html).toContain('Mock document');
    await expect(
      client.readDocxPreviewResource({
        sessionId: opened.sessionId,
        resourceId: resource?.resourceId ?? '',
      }),
    ).resolves.toMatchObject({ mediaType: 'image/png' });
    await client.closeDocxPreview({ sessionId: opened.sessionId });
    await expect(
      client.readDocxPreviewResource({
        sessionId: opened.sessionId,
        resourceId: resource?.resourceId ?? '',
      }),
    ).rejects.toMatchObject({ code: 'notFound' });
  });

  it('provides and cleans up the same rendered PPTX PDF session contract as other hosts', async () => {
    const client = new MockFileManagerClient();
    const opened = await client.openPptxPreview({
      location: { ...LOCATION, uri: 'mock:///briefing.pptx' },
    });
    expect(new TextDecoder().decode(new Uint8Array(opened.firstPagePdf))).toMatch(/^%PDF-/);
    const range = await client.readPptxPreviewPdf({
      sessionId: opened.sessionId,
      offset: 0,
      length: opened.firstPagePdf.length,
    });
    expect(new TextDecoder().decode(new Uint8Array(range.data))).toMatch(/^%PDF-/);
    await client.closePptxPreview({ sessionId: opened.sessionId });
    await expect(
      client.readPptxPreviewPdf({
        sessionId: opened.sessionId,
        offset: 0,
        length: 4,
      }),
    ).rejects.toMatchObject({ code: 'notFound' });
  });

  it('reads a bounded byte range and reports probablyBinary only at offset zero', async () => {
    const client = new MockFileManagerClient();

    const first = await client.readFileRange({ location: LOCATION, offset: 0, length: 16 });
    const second = await client.readFileRange({ location: LOCATION, offset: 16, length: 16 });

    expect(first.data).toHaveLength(16);
    expect(first.offset).toBe(0);
    expect(first.eof).toBe(false);
    expect(first.probablyBinary).toBe(false);
    expect(second.probablyBinary).toBeUndefined();
  });

  it('returns the same synthetic content across repeated reads of the same location', async () => {
    const client = new MockFileManagerClient();

    const first = await client.readFileRange({ location: LOCATION, offset: 0, length: 32 });
    const second = await client.readFileRange({ location: LOCATION, offset: 0, length: 32 });

    expect(second.data).toEqual(first.data);
  });

  it('reports eof once a range reaches the end of the synthetic content', async () => {
    const client = new MockFileManagerClient();
    const probe = await client.readFileRange({ location: LOCATION, offset: 0, length: 1 });
    // The synthetic content is deterministic per uri; find its end by requesting a huge range.
    const whole = await client.readFileRange({
      location: LOCATION,
      offset: 0,
      length: 10_000_000,
    });
    expect(whole.eof).toBe(true);
    expect(probe.eof).toBe(false);
  });

  it('rejects a zero-length range request', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.readFileRange({ location: LOCATION, offset: 0, length: 0 }),
    ).rejects.toMatchObject({ code: 'invalidRequest' });
  });

  it('finds case-insensitive substring matches by line', async () => {
    const client = new MockFileManagerClient();

    const result = await client.searchInFile({
      location: LOCATION,
      query: 'ERROR',
      regex: false,
      caseSensitive: false,
      wholeWord: false,
    });

    expect(result.matches.length).toBeGreaterThan(0);
    expect(result.matches[0]).toMatchObject({ length: 5 });
    expect(result.truncated).toBe(false);
  });

  it('finds regex matches', async () => {
    const client = new MockFileManagerClient();

    const result = await client.searchInFile({
      location: LOCATION,
      query: 'line \\d+ of',
      regex: true,
      caseSensitive: true,
      wholeWord: false,
    });

    expect(result.matches.length).toBeGreaterThan(0);
  });

  it('excludes matches inside a larger word when wholeWord is set', async () => {
    const client = new MockFileManagerClient();

    const partial = await client.searchInFile({
      location: LOCATION,
      query: 'err',
      regex: false,
      caseSensitive: false,
      wholeWord: false,
    });
    const wholeWord = await client.searchInFile({
      location: LOCATION,
      query: 'err',
      regex: false,
      caseSensitive: false,
      wholeWord: true,
    });

    expect(partial.matches.length).toBeGreaterThan(0);
    expect(wholeWord.matches).toHaveLength(0);
  });

  it('rejects an invalid regex query', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.searchInFile({
        location: LOCATION,
        query: '(',
        regex: true,
        caseSensitive: false,
        wholeWord: false,
      }),
    ).rejects.toMatchObject({ code: 'invalidRequest' });
  });

  it('rejects an empty search query', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.searchInFile({
        location: LOCATION,
        query: '',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
      }),
    ).rejects.toMatchObject({ code: 'invalidRequest' });
  });

  it('recursively sums a directory tree, descending into subdirectories', async () => {
    const client = new MockFileManagerClient();

    const result = await client.calculateFolderSize({
      location: { providerId: 'file', uri: 'mock:///Documents' },
    });

    // mock:///Documents/report.pdf (8192) + mock:///Documents/Projects/file-manager.md (2048)
    expect(result).toEqual({ totalBytes: 10_240, fileCount: 2 });
  });

  it('rejects a folder-size request for an unknown directory', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.calculateFolderSize({
        location: { providerId: 'file', uri: 'mock:///does-not-exist' },
      }),
    ).rejects.toMatchObject({ code: 'directoryNotFound' });
  });

  it('discovers a mock application bundle by its .app-suffixed name', async () => {
    const client = new MockFileManagerClient();

    const result = await client.discoverApplicationUninstallCandidates({
      location: { providerId: 'file', uri: 'mock:///Applications/Widget.app' },
    });

    expect(result).toEqual({
      bundleIdentifier: 'com.example.Widget',
      productName: 'Widget',
      relatedFiles: [],
    });
  });

  it('rejects application-uninstall discovery for a non-.app entry', async () => {
    const client = new MockFileManagerClient();

    await expect(
      client.discoverApplicationUninstallCandidates({
        location: { providerId: 'file', uri: 'mock:///Documents/report.pdf' },
      }),
    ).rejects.toMatchObject({ code: 'notFound' });
  });

  it('reports no pinned Dock icon to remove, since the mock world has no Dock', async () => {
    const client = new MockFileManagerClient();

    const result = await client.removeApplicationDockIcon({
      location: { providerId: 'file', uri: 'mock:///Applications/Widget.app' },
    });

    expect(result).toEqual({ removed: false });
  });
});
