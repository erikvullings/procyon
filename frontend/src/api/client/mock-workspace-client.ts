import { errorText } from '../../i18n';
import type {
  ArchiveCredentialRequest,
  ArchiveEdit,
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
  ResolveArchiveEditRequest,
  ResolveMetadataTokensRequest,
  ResolveMetadataTokensResult,
  SaveEditableFileRequest,
  ScanDiskUsageRequest,
  ScanDiskUsageResult,
  SearchInFileMatch,
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
import { encodeDiskUsageTree } from '../../models';
import type { MockContext } from './mock-context';
import {
  createMockWorkspace,
  directories,
  evaluateMockConnectionStatus,
  fixtureEntry,
  MockClientError,
  THUMBNAILABLE_MOCK_EXTENSIONS,
} from './mock-shared';
import { MockSqliteDatabase } from './mock-sqlite-database';
import type { WorkspaceClient } from './workspace-client';

export class MockWorkspaceClient implements WorkspaceClient {
  constructor(private readonly ctx: MockContext) {}

  cacheArchivePassword(_request: ArchiveCredentialRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cacheArchivePassword', signal, () => undefined);
  }

  listArchiveEdits(signal?: AbortSignal): Promise<ArchiveEdit[]> {
    return this.ctx.perform('listArchiveEdits', signal, () => []);
  }

  resolveArchiveEdit(_request: ResolveArchiveEditRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('resolveArchiveEdit', signal, () => undefined);
  }

  getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined> {
    return this.ctx.perform('getFileIcon', signal, () => {
      const pathname = new URL(sampleLocationUri).pathname;
      const name = pathname.slice(pathname.lastIndexOf('/') + 1);
      const extension = name.includes('.')
        ? name.slice(name.lastIndexOf('.') + 1).toLowerCase()
        : '';
      if (!this.ctx.nativeIconExtensions.has(extension)) return undefined;
      return new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    });
  }

  getThumbnail(
    locationUri: string,
    _size: 'small' | 'medium' | 'large',
    signal?: AbortSignal,
  ): Promise<Uint8Array | undefined> {
    return this.ctx.perform('getThumbnail', signal, () => {
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
    return this.ctx.perform(
      'getFinderTags',
      signal,
      () => this.ctx.finderTagsByUri.get(locationUri) ?? { tags: [] },
    );
  }

  setFinderTags(locationUri: string, tags: FinderTags, signal?: AbortSignal): Promise<FinderTags> {
    return this.ctx.perform('setFinderTags', signal, () => {
      const persisted = structuredClone(tags);
      this.ctx.finderTagsByUri.set(locationUri, persisted);
      return structuredClone(persisted);
    });
  }

  getSpotlightComment(
    locationUri: string,
    signal?: AbortSignal,
  ): Promise<SpotlightComment | undefined> {
    return this.ctx.perform(
      'getSpotlightComment',
      signal,
      () => this.ctx.spotlightCommentsByUri.get(locationUri) ?? { comment: null },
    );
  }

  setSpotlightComment(
    locationUri: string,
    comment: SpotlightComment,
    signal?: AbortSignal,
  ): Promise<SpotlightComment> {
    return this.ctx.perform('setSpotlightComment', signal, () => {
      const persisted = structuredClone(comment);
      this.ctx.spotlightCommentsByUri.set(locationUri, persisted);
      return structuredClone(persisted);
    });
  }

  listWorkspaces(signal?: AbortSignal): Promise<WorkspaceSummary[]> {
    return this.ctx.perform('listWorkspaces', signal, () =>
      [...this.ctx.workspaces.values()].map(({ id, name, revision, ephemeral }) => ({
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
    return this.ctx.perform('createWorkspace', signal, () => {
      this.ctx.workspaceSequence += 1;
      const workspace = createMockWorkspace(
        `mock-workspace-${this.ctx.workspaceSequence}`,
        request.name ?? 'Default',
      );
      this.ctx.workspaces.set(workspace.id, workspace);
      return structuredClone(workspace);
    });
  }

  startWorkspace(workspaceId?: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.ctx.perform('startWorkspace', signal, () => {
      const existing = workspaceId === undefined ? undefined : this.ctx.workspaces.get(workspaceId);
      if (existing !== undefined) return structuredClone(existing);
      const [first] = this.ctx.workspaces.values();
      if (first !== undefined) return structuredClone(first);
      this.ctx.workspaceSequence += 1;
      const workspace = createMockWorkspace(
        `mock-workspace-${this.ctx.workspaceSequence}`,
        'Default',
      );
      this.ctx.workspaces.set(workspace.id, workspace);
      return structuredClone(workspace);
    });
  }

  getWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.ctx.perform('getWorkspace', signal, () => {
      const workspace = this.ctx.workspaces.get(workspaceId) ?? createMockWorkspace(workspaceId);
      this.ctx.workspaces.set(workspaceId, workspace);
      return structuredClone(workspace);
    });
  }

  renameWorkspace(
    workspaceId: WorkspaceId,
    name: string,
    expectedRevision: number,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.ctx.workspaceClient.dispatchWorkspaceCommand(
      { type: 'renameWorkspace', workspaceId, name, expectedRevision },
      signal,
    );
  }

  deleteWorkspace(
    workspaceId: WorkspaceId,
    expectedRevision?: number,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.ctx.perform('deleteWorkspace', signal, () => {
      const workspace = this.ctx.workspaces.get(workspaceId);
      if (workspace !== undefined && expectedRevision !== undefined) {
        this.ctx.requireWorkspaceRevision(workspace, expectedRevision);
      }
      this.ctx.workspaces.delete(workspaceId);
      let changed = false;
      for (const root of this.ctx.semanticLibrary.roots) {
        const remaining = root.workspaceReferences.filter((id) => id !== workspaceId);
        if (remaining.length !== root.workspaceReferences.length) {
          root.workspaceReferences = remaining;
          changed = true;
        }
      }
      if (changed) this.ctx.advanceSemanticLibraryRevision();
    });
  }

  openWorkspace(workspaceId: WorkspaceId, signal?: AbortSignal): Promise<WorkspaceProjection> {
    return this.ctx.perform('openWorkspace', signal, () => {
      const workspace = this.ctx.workspaces.get(workspaceId) ?? createMockWorkspace(workspaceId);
      this.ctx.workspaces.set(workspaceId, workspace);
      return structuredClone(workspace);
    });
  }

  dispatchWorkspaceCommand(
    command: WorkspaceCommand,
    signal?: AbortSignal,
  ): Promise<WorkspaceProjection> {
    return this.ctx.perform('dispatchWorkspaceCommand', signal, () => {
      const current =
        this.ctx.workspaces.get(command.workspaceId) ?? createMockWorkspace(command.workspaceId);
      this.ctx.requireWorkspaceRevision(current, command.expectedRevision);
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
          this.ctx.tabSequence += 1;
          const tabId = `mock-tab-${this.ctx.tabSequence}`;
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
            this.ctx.tabSequence += 1;
            const replacementId = `mock-tab-${this.ctx.tabSequence}`;
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
          const history = this.ctx.navigationHistory.get(historyKey) ?? { back: [], forward: [] };
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
            this.ctx.navigationHistory.set(historyKey, history);
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
      this.ctx.workspaces.set(changed.id, changed);
      return structuredClone(changed);
    });
  }

  navigatePane(request: NavigateRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.ctx.directorySnapshot(request, signal, 'navigatePane');
  }

  listDirectory(request: ListDirectoryRequest, signal?: AbortSignal): Promise<DirectorySnapshot> {
    return this.ctx.directorySnapshot(request, signal, 'listDirectory');
  }

  listDirectoryChildren(
    location: Location,
    showHidden: boolean,
    signal?: AbortSignal,
  ): Promise<readonly EntrySummary[]> {
    return this.ctx.perform('listDirectoryChildren', signal, () => {
      const fixtures = directories[location.uri] ?? [];
      return fixtures
        .map((fixture) => fixtureEntry(location.uri, fixture))
        .filter((entry) => entry.kind === 'directory' && (showHidden || !entry.hidden));
    });
  }

  getEntryMetadata(request: EntryMetadataRequest, signal?: AbortSignal): Promise<EntryMetadata> {
    return this.ctx.perform('getEntryMetadata', signal, () => {
      if (request.verifyIdentity) {
        const actual = Object.entries(directories)
          .flatMap(([parent, fixtures]) => fixtures.map((fixture) => fixtureEntry(parent, fixture)))
          .find((entry) => entry.location.uri === request.location.uri);
        if (actual === undefined) throw new MockClientError('notFound', 'Entry no longer exists.');
        if (actual.id !== request.entryId) {
          throw new MockClientError('invalidRequest', 'Entry identity changed.', {
            reason: 'entryIdentityChanged',
          });
        }
        if (
          (request.expectedSize !== undefined && actual.size !== request.expectedSize) ||
          (request.expectedModifiedAt !== undefined &&
            actual.modifiedAt !== request.expectedModifiedAt)
        ) {
          throw new MockClientError('invalidRequest', 'Entry metadata changed.', {
            reason: 'entryMetadataChanged',
          });
        }
      }
      return {
        entryId: request.entryId,
        permissions: { readable: true, writable: true, executable: false },
        ownership: { owner: 'mock-user', group: 'mock-group' },
        extendedAttributes: {},
        checksums: {},
        pluginFields: {},
      };
    });
  }

  setPaneActivity(_request: SetPaneActivityRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('setPaneActivity', signal, () => undefined);
  }

  previewGeoConversion(
    locations: readonly Location[],
    format: GeoTargetFormat,
    _csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<GeoConversionReport[]> {
    return this.ctx.perform('previewGeoConversion', signal, () =>
      locations.map((location) => ({
        location,
        outputName: `converted.${format}`,
        notes:
          format === 'gpx'
            ? [{ code: 'polygonsNotInGpx', message: 'Polygons cannot be stored in GPX.' }]
            : [],
      })),
    );
  }

  previewGeoMinify(
    _location: Location,
    minify: GeoMinify,
    signal?: AbortSignal,
  ): Promise<GeoMinifyPreview> {
    return this.ctx.perform('previewGeoMinify', signal, () => ({
      originalBytes: 1000,
      minifiedBytes: minify.precision === undefined ? 900 : 400,
      featuresBefore: 1,
      featuresAfter: 1,
      positionsBefore: 10,
      positionsAfter: 5,
      distanceBeforeMeters: 1000,
      distanceAfterMeters: 990,
      notes: [],
    }));
  }

  convertGeoText(
    text: string,
    format: GeoTargetFormat,
    _csvWkt: boolean,
    signal?: AbortSignal,
  ): Promise<{ content: string; extension: string; notes: GeoMessage[] }> {
    return this.ctx.perform('convertGeoText', signal, () => ({
      content: text,
      extension: format,
      notes: [],
    }));
  }

  parseGeoText(text: string, signal?: AbortSignal): Promise<GeoDocument> {
    return this.ctx.perform('parseGeoText', signal, () => {
      const empty = { type: 'FeatureCollection' as const, features: [] };
      try {
        const value = JSON.parse(text) as { type?: string; features?: unknown[] };
        const features = (
          value.type === 'FeatureCollection' ? (value.features ?? []) : [value]
        ) as GeoJSON.Feature[];
        return {
          format: 'geojson',
          collection: { type: 'FeatureCollection' as const, features },
          totalFeatures: features.length,
          displayedFeatures: features.length,
          truncated: false,
          warnings: [],
          skipped: [],
        };
      } catch (error) {
        return {
          format: 'geojson',
          collection: empty,
          totalFeatures: 0,
          displayedFeatures: 0,
          truncated: false,
          warnings: [],
          skipped: [],
          error: {
            code: 'jsonSyntax',
            message: errorText(error),
            params: { line: '1', column: '1', detail: errorText(error) },
          },
        };
      }
    });
  }

  readGeoFile(_location: Location, signal?: AbortSignal): Promise<GeoDocument> {
    return this.ctx.perform('readGeoFile', signal, () => ({
      format: 'geojson',
      collection: {
        type: 'FeatureCollection',
        features: [
          {
            type: 'Feature',
            geometry: { type: 'Point', coordinates: [4.9, 52.37] },
            properties: { name: 'Amsterdam' },
          },
        ],
      },
      totalFeatures: 1,
      displayedFeatures: 1,
      truncated: false,
      bounds: [4.9, 52.37, 4.9, 52.37],
      warnings: [],
      skipped: [],
    }));
  }

  readImageGps(locations: readonly Location[], signal?: AbortSignal): Promise<ImageGpsEntry[]> {
    // Deterministic fake: every second image is geotagged around Amsterdam.
    return this.ctx.perform('readImageGps', signal, () =>
      locations.map((location, index) =>
        index % 2 === 0
          ? { location, gps: { latitude: 52.37 + index * 0.001, longitude: 4.89 + index * 0.001 } }
          : { location },
      ),
    );
  }

  inspectAttributes(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectAttributesResult> {
    return this.ctx.perform('inspectAttributes', signal, () => ({
      support: { mode: true, ownership: true, windowsAttributes: false },
      entries: locations.map((location) => ({
        location,
        kind: 'file' as const,
        attributes: { mode: 0o644, uid: 501, gid: 20 },
      })),
    }));
  }

  inspectSplitSet(
    locations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<InspectSplitSetResult> {
    return this.ctx.perform('inspectSplitSet', signal, () => {
      const names = locations.map(
        (location) => decodeURIComponent(location.uri).split('/').pop() ?? '',
      );
      const base = (names[0] ?? '').replace(/\.\d{3,}$/, '');
      if (names.length === 0 || base === names[0]) {
        return {
          outputName: base,
          parts: [],
          totalSize: 0,
          hasManifest: false,
          error: `${names[0] ?? ''} is not a numbered part (.001, .002, ...)`,
          errorCode: 'splitNotAPart',
          errorParams: { name: names[0] ?? '' },
        };
      }
      const parts = [1, 2, 3].map((index) => ({
        name: `${base}.${String(index).padStart(3, '0')}`,
        size: 1_048_576,
      }));
      return { outputName: base, parts, totalSize: 3 * 1_048_576, hasManifest: true };
    });
  }

  previewAttributeChange(
    locations: readonly Location[],
    recursive: boolean,
    scope: AttributeScope,
    signal?: AbortSignal,
  ): Promise<AttributeChangePreview> {
    return this.ctx.perform('previewAttributeChange', signal, () => {
      const folders = recursive && scope !== 'filesOnly' ? locations.length * 3 : 0;
      const files = scope === 'foldersOnly' ? 0 : locations.length * (recursive ? 12 : 1);
      return { total: files + folders, files, folders, skippedSymlinks: 0, truncated: false };
    });
  }

  readImageMetadata(_location: Location, signal?: AbortSignal): Promise<ImageMetadata> {
    return this.ctx.perform('readImageMetadata', signal, () => ({
      format: 'JPEG',
      supported: true,
      editableFields: [
        'dateTaken',
        'gps',
        'description',
        'copyright',
        'author',
        'orientation',
        'title',
        'keywords',
        'rating',
      ],
      groups: [
        {
          namespace: 'EXIF',
          tags: [
            { name: 'Make', value: 'Mock Camera Co.' },
            { name: 'Model', value: 'Mock 9000' },
          ],
        },
      ],
    }));
  }

  resolveMetadataTokens(
    request: ResolveMetadataTokensRequest,
    signal?: AbortSignal,
  ): Promise<ResolveMetadataTokensResult> {
    return this.ctx.perform('resolveMetadataTokens', signal, () => {
      const known = new Set(['artist', 'album', 'title', 'track', 'year']);
      const unknownTokens = request.tokens.filter(
        (token) => !known.has(token) && !token.startsWith('exif:'),
      );
      return {
        entries: request.locations.map((location, index) => ({
          location,
          values: Object.fromEntries(
            request.tokens
              .filter((token) => known.has(token))
              .map((token) => [
                token,
                token === 'track' ? String(index + 1).padStart(2, '0') : `Mock ${token}`,
              ]),
          ),
        })),
        unknownTokens,
      };
    });
  }

  mapTileUrlTemplate(): Promise<string> {
    // A flat grey tile: the mock runtime has no backend to proxy a provider.
    return Promise.resolve(
      'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mO89OLFfwAJVwP8GZfV0AAAAABJRU5ErkJggg==',
    );
  }

  mediaUrl(location: Location, signal?: AbortSignal): Promise<string> {
    return this.ctx.perform('readFileRange', signal, () => {
      const bytes = this.ctx.fileContentFor(location.uri);
      if (typeof URL.createObjectURL === 'function') {
        return URL.createObjectURL(new Blob([bytes as BlobPart]));
      }
      let binary = '';
      for (const byte of bytes) binary += String.fromCharCode(byte);
      return `data:application/octet-stream;base64,${btoa(binary)}`;
    });
  }

  readFileRange(request: ReadFileRangeRequest, signal?: AbortSignal): Promise<FileRangeChunk> {
    return this.ctx.perform('readFileRange', signal, () => {
      if (request.length <= 0) {
        throw new MockClientError('invalidRequest', 'length must be a positive number of bytes', {
          reason: 'invalidRangeLength',
          maxBytes: 1_048_576,
        });
      }
      const bytes = this.ctx.fileContentFor(request.location.uri);
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
    return this.ctx.perform('openDocxPreview', signal, () => {
      const sessionId = crypto.randomUUID();
      const resourceId = crypto.randomUUID();
      this.ctx.docxSessions.set(
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
    return this.ctx.perform('readDocxPreviewResource', signal, () => {
      const resource = this.ctx.docxSessions.get(request.sessionId)?.get(request.resourceId);
      if (resource === undefined) {
        throw new MockClientError('notFound', 'DOCX preview resource not found');
      }
      return structuredClone(resource);
    });
  }

  closeDocxPreview(request: DocxPreviewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('closeDocxPreview', signal, () => {
      if (!this.ctx.docxSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'DOCX preview session not found');
      }
    });
  }

  openPptxPreview(request: OpenPptxPreviewRequest, signal?: AbortSignal): Promise<PptxPreview> {
    return this.ctx.perform('openPptxPreview', signal, () => {
      const sessionId = crypto.randomUUID();
      const pdf = new TextEncoder().encode(
        '%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n' +
          '2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n' +
          '3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>endobj\n' +
          '4 0 obj<</Length 0>>stream\nendstream\nendobj\ntrailer<</Root 1 0 R>>\n%%EOF\n',
      );
      this.ctx.pptxSessions.set(sessionId, pdf);
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
    return this.ctx.perform('readPptxPreviewPdf', signal, () => {
      if (request.length <= 0) {
        throw new MockClientError('invalidRequest', 'length must be a positive number of bytes', {
          reason: 'invalidRangeLength',
          maxBytes: 1_048_576,
        });
      }
      const pdf = this.ctx.pptxSessions.get(request.sessionId);
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
    return this.ctx.perform('closePptxPreview', signal, () => {
      if (!this.ctx.pptxSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'PPTX preview session not found');
      }
    });
  }

  openStructuredView(
    request: OpenStructuredViewRequest,
    signal?: AbortSignal,
  ): Promise<StructuredView> {
    return this.ctx.perform('readFileRange', signal, () => {
      const sessionId = crypto.randomUUID();
      const delimiter = request.delimiter ?? (request.format === 'tsv' ? '\t' : ',');
      this.ctx.structuredSessions.set(sessionId, {
        uri: request.location.uri,
        format: request.format,
        delimiter,
        headerMode: request.headerMode ?? 'auto',
        selectedSheet: 'Summary',
      });
      return this.ctx.mockStructuredView(sessionId);
    });
  }

  getStructuredViewStatus(
    request: StructuredViewSessionRequest,
    signal?: AbortSignal,
  ): Promise<StructuredViewStatus> {
    return this.ctx.perform('readFileRange', signal, () => {
      const view = this.ctx.mockStructuredView(request.sessionId);
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
    return this.ctx.perform('readFileRange', signal, () => {
      const session = this.ctx.structuredSession(request.sessionId);
      if (request.delimiter != null) session.delimiter = request.delimiter;
      if (request.headerMode != null) session.headerMode = request.headerMode;
      if (request.selectedSheet != null) session.selectedSheet = request.selectedSheet;
      return this.ctx.mockStructuredView(request.sessionId);
    });
  }

  readStructuredRows(
    request: ReadStructuredRowsRequest,
    signal?: AbortSignal,
  ): Promise<StructuredRows> {
    return this.ctx.perform('readFileRange', signal, () => {
      const view = this.ctx.mockStructuredView(request.sessionId);
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
    return this.ctx.perform('readFileRange', signal, () => {
      const session = this.ctx.structuredSession(request.sessionId);
      const bytes = this.ctx.fileContentFor(session.uri);
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
    return this.ctx.perform('readFileRange', signal, () => {
      const rows = this.ctx.mockStructuredView(request.sessionId).rows;
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
    return this.ctx.perform('readFileRange', signal, () => {
      if (!this.ctx.structuredSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'structured viewer session not found');
      }
    });
  }

  openSqliteView(request: OpenSqliteViewRequest, signal?: AbortSignal): Promise<SqliteView> {
    return this.ctx.perform('readFileRange', signal, () => {
      const sessionId = crypto.randomUUID();
      const database = new MockSqliteDatabase();
      this.ctx.sqliteSessions.set(sessionId, database);
      return {
        sessionId,
        sourceBytes: this.ctx.fileContentFor(request.location.uri).length,
        sqliteVersion: 'mock',
        objects: [...database.objects],
      };
    });
  }

  querySqliteView(
    request: QuerySqliteViewRequest,
    signal?: AbortSignal,
  ): Promise<SqliteQueryResult> {
    return this.ctx.perform('readFileRange', signal, () => {
      const database = this.ctx.sqliteSessions.get(request.sessionId);
      if (database === undefined) {
        throw new MockClientError('notFound', 'SQLite viewer session not found');
      }
      try {
        return database.query(request.source, request.offset ?? 0, request.limit);
      } catch (error) {
        throw new MockClientError(
          'invalidRequest',
          error instanceof Error ? error.message : 'query failed',
        );
      }
    });
  }

  closeSqliteView(request: SqliteViewSessionRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('readFileRange', signal, () => {
      if (!this.ctx.sqliteSessions.delete(request.sessionId)) {
        throw new MockClientError('notFound', 'SQLite viewer session not found');
      }
    });
  }

  loadEditableFile(request: LoadEditableFileRequest, signal?: AbortSignal): Promise<EditableFile> {
    return this.ctx.perform('readFileRange', signal, () => {
      const bytes = this.ctx.fileContentFor(request.location.uri);
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
    return this.ctx.perform('readFileRange', signal, () => {
      const existing = this.ctx.fileContentFor(request.location.uri);
      if (!request.overwriteConflict && request.expectedRevision !== String(existing.length)) {
        throw new MockClientError('fileRevisionConflict', 'The file changed after it was loaded.');
      }
      const bytes = new TextEncoder().encode(request.content);
      this.ctx.fileContents.set(request.destination?.uri ?? request.location.uri, bytes);
      return {
        revision: String(bytes.length),
        size: bytes.length,
        overwroteConflict: request.overwriteConflict,
      };
    });
  }

  searchInFile(request: SearchInFileRequest, signal?: AbortSignal): Promise<SearchInFileResult> {
    return this.ctx.perform('searchInFile', signal, () => {
      if (request.query.length === 0) {
        throw new MockClientError('invalidRequest', 'search query must not be empty', {
          reason: 'searchQueryInvalid',
        });
      }
      const matchesOnLine = this.ctx.buildLineMatcher(request);
      const text = new TextDecoder().decode(this.ctx.fileContentFor(request.location.uri));
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
    return this.ctx.perform('calculateFolderSize', signal, () => {
      if (directories[request.location.uri] === undefined) {
        throw new MockClientError('notFound', `No mock directory at ${request.location.uri}`);
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
    return this.ctx.perform('archiveSummary', signal, () => {
      const archiveRootUri = request.location.uri.startsWith('file://')
        ? `archive://${request.location.uri.slice('file://'.length)}!/`
        : request.location.uri;
      const rootEntries = directories[archiveRootUri];
      if (rootEntries === undefined) {
        throw new MockClientError('notFound', `No mock archive at ${request.location.uri}`);
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
    return this.ctx.perform('scanDiskUsage', signal, () => {
      const fixtures = directories[request.location.uri];
      if (fixtures === undefined) {
        throw new MockClientError('notFound', `No mock directory at ${request.location.uri}`);
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
      const root = build(request.location.uri, name, true);
      // Mirrors the backend's clean-up heuristics for the one rule the fixtures can exercise; the
      // backend's 50 MB threshold is ignored because fixture sizes are tiny.
      const cleanupCandidates: NonNullable<ScanDiskUsageResult['cleanupCandidates']> = [];
      const collectCandidates = (node: ScanDiskUsageResult['root']): void => {
        for (const child of node.children) {
          if (child.kind !== 'directory') continue;
          if (child.name === 'node_modules') {
            cleanupCandidates.push({
              location: child.location,
              kind: 'nodeModules',
              logicalBytes: child.logicalBytes,
              physicalBytes: child.physicalBytes,
            });
          } else {
            collectCandidates(child);
          }
        }
      };
      collectCandidates(root);
      cleanupCandidates.sort((left, right) => right.physicalBytes - left.physicalBytes);
      const result = {
        root,
        unreadableEntries: 0,
        unreadable: [],
        scannedEntries: 1,
      };
      const first = result.root.children.at(0);
      if (first !== undefined) {
        this.ctx.eventSequence += 1;
        this.ctx.emit({
          eventId: this.ctx.eventSequence,
          timestamp: new Date().toISOString(),
          workspaceId: request.workspaceId as WorkspaceId,
          payload: {
            type: 'diskUsage.progress',
            scanId: request.scanId,
            tree: encodeDiskUsageTree({
              ...result.root,
              logicalBytes: first.logicalBytes,
              physicalBytes: first.physicalBytes,
              children: [first],
            }),
            unreadableEntries: 0,
            unreadable: [],
            scannedEntries: 1,
            isComplete: false,
          },
        });
      }
      this.ctx.eventSequence += 1;
      this.ctx.emit({
        eventId: this.ctx.eventSequence,
        timestamp: new Date().toISOString(),
        workspaceId: request.workspaceId as WorkspaceId,
        payload: {
          type: 'diskUsage.progress',
          scanId: request.scanId,
          tree: encodeDiskUsageTree(result.root),
          unreadableEntries: 0,
          unreadable: [],
          scannedEntries: 1,
          cleanupCandidates,
          isComplete: true,
        },
      });
      return undefined;
    });
  }

  cancelDiskUsage(_scanId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelDiskUsage', signal, () => undefined);
  }

  discoverApplicationUninstallCandidates(
    request: DiscoverApplicationUninstallCandidatesRequest,
    signal?: AbortSignal,
  ): Promise<DiscoverApplicationUninstallCandidatesResult> {
    return this.ctx.perform('discoverApplicationUninstallCandidates', signal, () => {
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
    return this.ctx.perform('removeApplicationDockIcon', signal, () => ({ removed: false }));
  }

  gitFileHistory(
    request: GitFileHistoryRequest,
    signal?: AbortSignal,
  ): Promise<GitFileHistoryResult> {
    // The mock fixtures have no notion of a git working tree, so every file simply has no
    // history to show - the same outcome a real backend reports for a non-git directory.
    void request;
    return this.ctx.perform('gitFileHistory', signal, () => ({ commits: [] }));
  }

  listConnections(signal?: AbortSignal): Promise<Connection[]> {
    return this.ctx.perform('listConnections', signal, () =>
      [...this.ctx.connections.values()].map((connection) => structuredClone(connection)),
    );
  }

  createConnection(request: CreateConnectionRequest, signal?: AbortSignal): Promise<Connection> {
    return this.ctx.perform('createConnection', signal, () => {
      this.ctx.connectionSequence += 1;
      const now = '2026-01-01T00:00:00.000Z';
      const connection: Connection = {
        id: `mock-connection-${this.ctx.connectionSequence}`,
        name: request.name,
        kind: request.kind,
        configuration: request.configuration,
        hasCredential: request.secret != null,
        status: 'disconnected',
        rootLocation:
          request.kind === 'oneDrive'
            ? `onedrive://mock-connection-${this.ctx.connectionSequence}/`
            : null,
        createdAt: now,
        updatedAt: now,
      };
      this.ctx.connections.set(connection.id, connection);
      return structuredClone(connection);
    });
  }

  getConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.ctx.perform('getConnection', signal, () =>
      structuredClone(this.ctx.requireConnection(connectionId)),
    );
  }

  updateConnection(
    connectionId: ConnectionId,
    request: UpdateConnectionRequest,
    signal?: AbortSignal,
  ): Promise<Connection> {
    return this.ctx.perform('updateConnection', signal, () => {
      const existing = this.ctx.requireConnection(connectionId);
      const updated: Connection = {
        ...existing,
        name: request.name,
        kind: request.kind,
        configuration: request.configuration,
        hasCredential: request.secret != null ? true : existing.hasCredential,
        updatedAt: '2026-01-01T00:00:00.000Z',
      };
      this.ctx.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  deleteConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('deleteConnection', signal, () => {
      this.ctx.requireConnection(connectionId);
      this.ctx.connections.delete(connectionId);
    });
  }

  connectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.ctx.perform('connectConnection', signal, () => {
      const connection = this.ctx.requireConnection(connectionId);
      const updated: Connection = {
        ...connection,
        status: evaluateMockConnectionStatus(connection),
      };
      this.ctx.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  disconnectConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.ctx.perform('disconnectConnection', signal, () => {
      const connection = this.ctx.requireConnection(connectionId);
      const updated: Connection = { ...connection, status: 'disconnected' };
      this.ctx.connections.set(connectionId, updated);
      return structuredClone(updated);
    });
  }

  /** Evaluates status without persisting it, mirroring the backend's `test` semantics. */
  testConnection(connectionId: ConnectionId, signal?: AbortSignal): Promise<Connection> {
    return this.ctx.perform('testConnection', signal, () => {
      const connection = this.ctx.requireConnection(connectionId);
      return structuredClone({ ...connection, status: evaluateMockConnectionStatus(connection) });
    });
  }

  beginOneDriveAuthorization(
    connectionId: ConnectionId,
    signal?: AbortSignal,
  ): Promise<BeginOneDriveAuthorizationResponse> {
    return this.ctx.perform('beginOneDriveAuthorization', signal, () => {
      const connection = this.ctx.requireConnection(connectionId);
      if (connection.configuration.kind !== 'oneDrive') {
        throw new MockClientError('invalidRequest', 'Only OneDrive connections can be authorized', {
          reason: 'notOnedriveConnection',
        });
      }
      this.ctx.oneDriveAuthorizationSequence += 1;
      const attemptId = `mock-onedrive-authorization-${this.ctx.oneDriveAuthorizationSequence}`;
      this.ctx.oneDriveAuthorizations.set(attemptId, {
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
    return this.ctx.perform('getOneDriveAuthorizationAttempt', signal, () => {
      const authorization = this.ctx.oneDriveAuthorizations.get(attemptId);
      if (authorization === undefined) {
        throw new MockClientError('notFound', `No mock OneDrive authorization ${attemptId}`);
      }
      if (authorization.attempt.status.state === 'pending') {
        const connection = this.ctx.requireConnection(authorization.connectionId);
        if (connection.configuration.kind !== 'oneDrive') {
          throw new MockClientError(
            'invalidRequest',
            'Connection kind changed during authorization',
            { reason: 'notOnedriveConnection' },
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
        this.ctx.connections.set(connection.id, authorized);
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
    return this.ctx.perform('cancelOneDriveAuthorization', signal, () => {
      const authorization = this.ctx.oneDriveAuthorizations.get(attemptId);
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
    return this.ctx.perform('probeSshHostKey', signal, () => {
      this.ctx.requireConnection(connectionId);
      return { status: 'trusted', fingerprint: 'SHA256:mock-fingerprint' };
    });
  }

  acceptSshHostKey(
    connectionId: ConnectionId,
    _fingerprint: string,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.ctx.perform('acceptSshHostKey', signal, () => {
      this.ctx.requireConnection(connectionId);
    });
  }
}
