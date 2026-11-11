import type {
  AppUpdateCheck,
  AppUpdateCheckOptions,
  AppUpdateProgress,
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
  WorkspaceId,
  WorkspaceProjection,
} from '../../models';
import type { EventStreamStatusObservable } from '../events/event-stream';
import type { NativeFileDrop, OpenWithApplication, WindowChrome } from './client-types';

/**
 * Transport, host-lifecycle, platform and event-stream concerns (spec §12).
 */
export interface ConnectionClient {
  checkForAppUpdate(signal?: AbortSignal, options?: AppUpdateCheckOptions): Promise<AppUpdateCheck>;

  disconnect(): void;

  /** Version of the running desktop app, or `undefined` outside the desktop host. */
  getAppVersion(signal?: AbortSignal): Promise<string | undefined>;

  /** Diagnostics view for troubleshooting and bug reports (spec §30). */
  getDiagnostics(signal?: AbortSignal): Promise<DiagnosticsResult>;

  /** The current user's home directory as a native path, for expanding a leading `~` typed
   * into an address bar. `undefined` where the host can't determine one (e.g. networked/browser
   * hosting). */
  getHomeDirectory(signal?: AbortSignal): Promise<string | undefined>;

  getRuntimeCapabilities(signal?: AbortSignal): Promise<RuntimeCapabilities>;

  getSettings(signal?: AbortSignal): Promise<Settings>;

  /** Templates for "New file from template" across app, folder and plugin sources (task 0312). */
  listFileTemplates(signal?: AbortSignal): Promise<FileTemplateEntry[]>;

  getSystemLocations(signal?: AbortSignal): Promise<SystemLocation[]>;

  getVolumes(signal?: AbortSignal): Promise<Volume[]>;

  /** Downloads, verifies, installs, and restarts into the most recently checked update. */
  installAppUpdate(
    onProgress?: (progress: AppUpdateProgress) => void,
    signal?: AbortSignal,
  ): Promise<void>;

  /** Desktop-only Launch Services applications for a local file's Open With submenu. */
  listOpenWithApplications(location: Location): Promise<OpenWithApplication[]>;

  onResynchronise(listener: () => void): Unsubscribe;

  /** Opens a user-confirmed external URL in the host's default browser. */
  openExternalUrl(url: string): Promise<void>;

  /** Opens the F3 viewer for one file in its own OS window; `payload` is the URL-safe viewer
   * request token. Desktop-only: the browser host opens a popup of the same page instead. */
  openViewerWindow?(payload: string, title: string): Promise<void>;

  openWithAnyApplication(location: Location): Promise<void>;

  openWithApplication(location: Location, applicationPath: string): Promise<void>;

  /** Opens a new OS window on its own private, disposable workspace forked from
   * `sourceWorkspaceId`'s current shape - or the hardcoded default shape if omitted, when there is
   * no named workspace to fork from yet (ephemeral per-window workspaces spec follow-up).
   * Desktop-only, like {@link quit}: the browser/HTTP host has no window concept. */
  openWorkspaceWindow?(sourceWorkspaceId?: WorkspaceId): Promise<void>;

  /** Closes the desktop window (Alt+F4, task 0128). Only implemented on the Tauri host. */
  quit?(): Promise<void>;

  /** Local files referenced on the operating system clipboard (e.g. copied in Finder or
   * Explorer); empty when there are none or the host has no OS file clipboard. */
  readOsClipboardFiles(signal?: AbortSignal): Promise<Location[]>;

  /** Retains a redacted frontend error in the host diagnostics buffer (task 0224). */
  recordFrontendDiagnostic(error: FrontendDiagnostic, signal?: AbortSignal): Promise<void>;

  /** Checks the host's signed update endpoint: stable, or stable plus beta releases. */
  /** Deletes settings, workspaces, semantic index data, logs and caches after the app quits. */
  removeApplicationData(): Promise<void>;

  /** Writes an ephemeral (per-window) workspace's current tabs/panes/layout back into
   * `targetWorkspaceId`, or - if omitted - the named workspace it was forked from, creating one
   * if it was seeded from the hardcoded default and has no source yet (ephemeral per-window
   * workspaces spec follow-up). `targetWorkspaceId` lets the workspace switcher's per-row
   * "Update" button replace any saved workspace's content with the current session's tabs,
   * keeping that workspace's own name. Returns the target named workspace, not the ephemeral
   * one. Desktop-only, like {@link openWorkspaceWindow}. */
  resyncWorkspace?(
    ephemeralWorkspaceId: WorkspaceId,
    targetWorkspaceId?: WorkspaceId,
  ): Promise<WorkspaceProjection>;

  /** Applies theme-derived chrome colours to the host window. Desktop-only; absent elsewhere. */
  setWindowChrome?(chrome: WindowChrome): Promise<void>;

  /** Sets the host window title (what the macOS Dock menu lists). Never rejects. */
  setWindowTitle?(title: string): Promise<void>;

  /** Opens the native Services (macOS) or Send To (Windows) submenu for a local selection. */
  showPlatformContextMenu(locations: readonly Location[], signal?: AbortSignal): Promise<void>;

  /** Starts an OS file-reference drag from the desktop host. */
  startNativeDrag(locations: readonly Location[], signal?: AbortSignal): Promise<void>;

  subscribe(listener: (event: BackendEvent) => void): Promise<Unsubscribe>;

  /** Subscribes to Finder/Explorer file drops over the desktop window. */
  subscribeNativeFileDrops(listener: (drop: NativeFileDrop) => void): Promise<Unsubscribe>;

  updateSettings(settings: Settings, signal?: AbortSignal): Promise<Settings>;

  /** Publishes local files on the operating system clipboard as file references, so they paste
   * in Finder or Explorer. Resolves `false`, leaving the OS clipboard untouched, when the host
   * has no OS file clipboard (`RuntimeCapabilities.osFileClipboard`) or any entry is not a local
   * file. Always a copy on the OS side: cut/move semantics stay inside Procyon. */
  writeOsClipboardFiles(locations: readonly Location[], signal?: AbortSignal): Promise<boolean>;

  readonly connection: EventStreamStatusObservable;

  readonly supportsAppUpdates: boolean;

  readonly supportsAppDataRemoval: boolean;
}
