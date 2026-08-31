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
