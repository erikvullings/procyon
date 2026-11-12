import { appTemplateEntries } from '../../features/templates/file-templates';
import type {
  BackendEvent,
  DiagnosticsResult,
  FileTemplateEntry,
  FrontendDiagnostic,
  Location,
  RuntimeCapabilities,
  Settings,
  SystemLocation,
  Unsubscribe,
  Volume,
} from '../../models';
import { sanitizeFrontendDiagnostic } from '../../models';
import { ApiError, desktopHostRequiredError } from '../fetch-mutator';
import type { ConnectionClient } from './connection-client';
import type { NativeFileDrop } from './file-manager-client';
import type { MockContext } from './mock-context';

export class MockConnectionClient implements ConnectionClient {
  constructor(private readonly ctx: MockContext) {}

  async setWindowTitle(title: string): Promise<void> {
    document.title = title;
  }

  async openExternalUrl(url: string): Promise<void> {
    const opened = globalThis.open(url, '_blank', 'noopener,noreferrer');
    if (opened === null) {
      throw new ApiError(0, {
        code: 'externalLinkBlocked',
        message: 'The browser blocked the external link.',
      });
    }
    opened.opener = null;
  }

  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities> {
    return this.ctx.perform('getRuntimeCapabilities', signal, () => ({
      clipboard: false,
      extendedAttributes: true,
      finderAliases: false,
      osFileClipboard: false,
      finderTags: true,
      nativeDragOut: false,
      nativeFileIcons: this.ctx.nativeIconExtensions.size > 0,
      nativeMenus: false,
      platformContextMenu: false,
      nativeThumbnails: false,
      openTerminal: false,
      platform: 'linux',
      plugins: true,
      revealInSystemFileManager: false,
      runtime: 'mock',
      freshInstall: false,
      semanticComponentAuthority:
        this.ctx.semanticStatus.lifecycle.state === 'unavailable'
          ? 'unavailable'
          : 'deterministicMock',
      semanticRuntimeExecutableDownload:
        this.ctx.semanticStatus.lifecycle.state === 'unavailable' ? 'unavailable' : 'simulated',
      serverAdministration: false,
      systemTrash: false,
    }));
  }

  getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]> {
    return this.ctx.perform('getSystemLocations', signal, () => []);
  }

  getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult> {
    return this.ctx.perform('getDiagnostics', signal, () => ({
      frontendVersion: '0.1.0',
      backendVersion: '0.1.0',
      platform: 'Mock',
      runtimeCapabilities: {
        clipboard: false,
        freshInstall: false,
        extendedAttributes: true,
        finderAliases: false,
        osFileClipboard: false,
        finderTags: true,
        nativeDragOut: false,
        nativeFileIcons: this.ctx.nativeIconExtensions.size > 0,
        nativeMenus: false,
        platformContextMenu: false,
        nativeThumbnails: false,
        openTerminal: false,
        platform: 'linux',
        plugins: true,
        revealInSystemFileManager: false,
        runtime: 'mock',
        semanticComponentAuthority:
          this.ctx.semanticStatus.lifecycle.state === 'unavailable'
            ? 'unavailable'
            : 'deterministicMock',
        semanticRuntimeExecutableDownload:
          this.ctx.semanticStatus.lifecycle.state === 'unavailable' ? 'unavailable' : 'simulated',
        serverAdministration: false,
        systemTrash: false,
      },
      connectionState: {
        connected: true,
        uptimeSeconds: 0,
        eventsReceived: 0,
        statusMessage: 'Mock',
      },
      loadedPlugins: [],
      recentErrors: structuredClone(this.ctx.frontendDiagnostics),
      operationQueueStatus: {
        queuedCount: 0,
        runningCount: 0,
        pausedCount: 0,
        completedCount: 0,
        totalPendingSize: 0,
      },
    }));
  }

  recordFrontendDiagnostic(error: FrontendDiagnostic, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('recordFrontendDiagnostic', signal, () => {
      this.ctx.frontendDiagnostics.unshift(sanitizeFrontendDiagnostic(error));
      this.ctx.frontendDiagnostics.splice(50);
    });
  }

  getVolumes(signal?: AbortSignal): Promise<Volume[]> {
    return this.ctx.perform('getVolumes', signal, () => [
      { name: 'Macintosh HD', location: { providerId: 'file', uri: 'mock:///' } },
      { name: 'Empty Drive', location: { providerId: 'file', uri: 'mock:///Empty' } },
    ]);
  }

  getHomeDirectory(signal?: AbortSignal): Promise<string | undefined> {
    return this.ctx.perform('getHomeDirectory', signal, () => '/Users/mock');
  }

  startNativeDrag(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('startNativeDrag', signal, () => undefined);
  }

  /** The mock host has no OS file clipboard (`osFileClipboard: false`). */
  writeOsClipboardFiles(_locations: readonly Location[], signal?: AbortSignal): Promise<boolean> {
    return this.ctx.perform('writeOsClipboardFiles', signal, () => false);
  }

  readOsClipboardFiles(signal?: AbortSignal): Promise<Location[]> {
    return this.ctx.perform('readOsClipboardFiles', signal, () => []);
  }

  showPlatformContextMenu(_locations: readonly Location[], signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('showPlatformContextMenu', signal, () => undefined);
  }

  listOpenWithApplications(
    _location: Location,
  ): Promise<import('./file-manager-client').OpenWithApplication[]> {
    return Promise.reject(desktopHostRequiredError());
  }

  openWithApplication(_location: Location, _applicationPath: string): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  openWithAnyApplication(_location: Location): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  subscribeNativeFileDrops(_listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe> {
    return Promise.resolve(() => undefined);
  }

  getSettings(signal?: AbortSignal): Promise<Settings> {
    return this.ctx.perform('getSettings', signal, () => structuredClone(this.ctx.settings));
  }

  listFileTemplates(signal?: AbortSignal): Promise<FileTemplateEntry[]> {
    return this.ctx.perform('listFileTemplates', signal, () =>
      appTemplateEntries(this.ctx.settings.fileTemplates),
    );
  }

  updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings> {
    return this.ctx.perform('updateSettings', signal, () => {
      this.ctx.settings = structuredClone(settings);
      return structuredClone(this.ctx.settings);
    });
  }

  getAppVersion(_signal?: AbortSignal): Promise<undefined> {
    return Promise.resolve(undefined);
  }

  checkForAppUpdate(_signal?: AbortSignal): Promise<never> {
    return Promise.reject(desktopHostRequiredError());
  }

  removeApplicationData(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  installAppUpdate(): Promise<void> {
    return Promise.reject(desktopHostRequiredError());
  }

  subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe> {
    this.ctx.connection.set('open');
    this.ctx.listeners.add(listener);
    return Promise.resolve(() => {
      this.ctx.listeners.delete(listener);
    });
  }

  disconnect(): void {
    this.ctx.connection.set('closed');
  }

  onResynchronise(listener: () => void): Unsubscribe {
    return this.ctx.resynchronise.subscribe(listener);
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
}
