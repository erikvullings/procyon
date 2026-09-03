/** Diagnostics model and types (spec §30). */

import type { RuntimeCapabilities } from '../../models/runtime-capabilities';

/** Diagnostics information for troubleshooting and bug reports. */
export interface DiagnosticsView {
  frontendVersion: string;
  backendVersion: string;
  tauriVersion?: string;
  platform: string;
  runtimeCapabilities: RuntimeCapabilities;
  connectionState: ConnectionState;
  loadedPlugins: PluginStatus[];
  recentErrors: DiagnosticError[];
  operationQueueStatus: OperationQueueStatus;
}

/** State of the SSE/event channel connection. */
export interface ConnectionState {
  connected: boolean;
  lastEventReceived?: string; // ISO 8601 timestamp
  uptimeSeconds: number;
  eventsReceived: number;
  statusMessage: string;
}

/** Plugin status in the diagnostics view. */
export interface PluginStatus {
  pluginId: string;
  name: string;
  enabled: boolean;
  version: string;
  errorCount: number;
}

/** A single error entry from the recent errors buffer. */
export interface DiagnosticError {
  timestamp: string; // ISO 8601
  message: string;
  code: string;
  context?: string;
}

/** Operation queue status for diagnostics. */
export interface OperationQueueStatus {
  queuedCount: number;
  runningCount: number;
  pausedCount: number;
  completedCount: number;
  totalPendingSize: number;
}

interface DiagnosticsDto
  extends Partial<
    Omit<
      DiagnosticsView,
      'connectionState' | 'loadedPlugins' | 'recentErrors' | 'operationQueueStatus'
    >
  > {
  connectionState?: Partial<ConnectionState>;
  loadedPlugins?: PluginStatus[] | null;
  recentErrors?: DiagnosticError[] | null;
  operationQueueStatus?: Partial<OperationQueueStatus>;
}

/** Convert DTO to domain model. */
export function diagnosticsFromDto(input: unknown): DiagnosticsView {
  const dto =
    typeof input === 'object' && input !== null
      ? (input as DiagnosticsDto)
      : ({} as DiagnosticsDto);
  return {
    frontendVersion: dto.frontendVersion ?? '',
    backendVersion: dto.backendVersion ?? '',
    ...(dto.tauriVersion === undefined ? {} : { tauriVersion: dto.tauriVersion }),
    platform: dto.platform ?? 'Unknown',
    runtimeCapabilities: dto.runtimeCapabilities ?? {
      runtime: 'browserServer',
      platform: 'unknown',
      nativeMenus: false,
      platformContextMenu: false,
      nativeFileIcons: false,
      nativeThumbnails: false,
      nativeDragOut: false,
      systemTrash: false,
      revealInSystemFileManager: false,
      openTerminal: false,
      clipboard: false,
      plugins: false,
      serverAdministration: false,
      extendedAttributes: false,
      finderAliases: false,
      finderTags: false,
    },
    connectionState: {
      connected: dto.connectionState?.connected ?? false,
      ...(dto.connectionState?.lastEventReceived === undefined
        ? {}
        : { lastEventReceived: dto.connectionState.lastEventReceived }),
      uptimeSeconds: dto.connectionState?.uptimeSeconds ?? 0,
      eventsReceived: dto.connectionState?.eventsReceived ?? 0,
      statusMessage: dto.connectionState?.statusMessage ?? 'Unknown',
    },
    loadedPlugins: (dto.loadedPlugins ?? []).map((p) => ({
      pluginId: p.pluginId,
      name: p.name,
      enabled: p.enabled,
      version: p.version,
      errorCount: p.errorCount,
    })),
    recentErrors: (dto.recentErrors ?? []).map((e) => ({
      timestamp: e.timestamp,
      message: e.message,
      code: e.code,
      ...(e.context === undefined ? {} : { context: e.context }),
    })),
    operationQueueStatus: {
      queuedCount: dto.operationQueueStatus?.queuedCount ?? 0,
      runningCount: dto.operationQueueStatus?.runningCount ?? 0,
      pausedCount: dto.operationQueueStatus?.pausedCount ?? 0,
      completedCount: dto.operationQueueStatus?.completedCount ?? 0,
      totalPendingSize: dto.operationQueueStatus?.totalPendingSize ?? 0,
    },
  };
}
