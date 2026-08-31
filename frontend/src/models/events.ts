import type { ChecksumEntry, DuplicateGroup } from './checksum';
import type { ComparisonEntry } from './comparison';
import type { ConnectionStatus } from './connection';
import type { ScanDiskUsageResult } from './disk-usage';
import type { EntrySummary } from './entry';
import type { ConnectionId, OperationId, PaneId, TabId, WorkspaceId } from './ids';
import type { Location } from './location';
import type { Operation, OperationProgress, OperationState } from './operation';
import type { PluginSummary } from './plugin';
import type { SearchExecutionMode } from './search';
import type { DirectoryDelta, DirectorySnapshot } from './snapshot';
import type { DirectoryViewConfiguration, WorkspaceLayout } from './workspace';

/** Shared tagged event envelope for backend-to-frontend events (spec §10). */
export interface EventEnvelope<T> {
  eventId: number;
  timestamp: string;
  workspaceId?: WorkspaceId;
  payload: T;
}

/** Conflict requiring an explicit request/response resolution. */
export interface OperationConflict {
  operationId: OperationId;
  conflictId: string;
  message: string;
  source: ConflictEntryMetadata;
  destination: ConflictEntryMetadata;
}

export interface ConflictEntryMetadata {
  name: string;
  size?: number;
  modifiedAt?: string;
  kind: 'file' | 'directory' | 'symlink';
}

/** User-visible notification delivered by the backend. */
export interface BackendNotification {
  id: string;
  level: 'info' | 'warning' | 'error';
  message: string;
}

/** Typed payloads named by specification §10. */
export type BackendEventPayload =
  | { type: 'runtime.ready' }
  | { type: 'workspace.created'; revision: number }
  | { type: 'workspace.renamed'; revision: number; name: string }
  | { type: 'workspace.opened'; revision: number }
  | { type: 'workspace.closed'; revision: number }
  | { type: 'workspace.deleted'; revision: number }
  | { type: 'workspace.layoutChanged'; revision: number; layout: WorkspaceLayout }
  | { type: 'workspace.activePaneChanged'; revision: number; paneId: PaneId }
  | {
      type: 'workspace.tabAdded';
      revision: number;
      paneId: PaneId;
      tabId: TabId;
      location: Location;
    }
  | { type: 'workspace.tabClosed'; revision: number; paneId: PaneId; tabId: TabId }
  | { type: 'workspace.tabActivated'; revision: number; paneId: PaneId; tabId: TabId }
  | {
      type: 'workspace.tabNavigated';
      revision: number;
      paneId: PaneId;
      tabId: TabId;
      location: Location;
    }
  | {
      type: 'workspace.tabViewChanged';
      revision: number;
      paneId: PaneId;
      tabId: TabId;
      view: DirectoryViewConfiguration;
    }
  | { type: 'directory.snapshot'; snapshot: DirectorySnapshot }
  | { type: 'directory.delta'; paneId: string; delta: DirectoryDelta }
  | { type: 'operation.created'; operation: Operation }
  | { type: 'operation.progress'; operationId: OperationId; progress: OperationProgress }
  | { type: 'operation.stateChanged'; operationId: OperationId; state: OperationState }
  | ({ type: 'operation.conflict' } & OperationConflict)
  | { type: 'operation.completed'; operation: Operation }
  | {
      type: 'operation.failed';
      operationId: OperationId;
      code: string;
      message: string;
      details?: Readonly<Record<string, unknown>>;
    }
  | { type: 'plugin.changed'; plugin: PluginSummary }
  | { type: 'notification.created'; notification: BackendNotification }
  | {
      type: 'search.resultsBatch';
      searchId: string;
      entries: EntrySummary[];
      isComplete: boolean;
      warningsCount: number;
      executionMode: SearchExecutionMode;
    }
  | {
      type: 'comparison.resultsBatch';
      comparisonId: string;
      entries: ComparisonEntry[];
      isComplete: boolean;
      warningsCount: number;
    }
  | {
      type: 'checksum.resultsBatch';
      jobId: string;
      entries: ChecksumEntry[];
      isComplete: boolean;
      isCancelled: boolean;
    }
  | {
      type: 'duplicates.resultsReady';
      scanId: string;
      groups: DuplicateGroup[];
      isCancelled: boolean;
      warningsCount: number;
    }
  | {
      type: 'diskUsage.progress';
      scanId: string;
      root: ScanDiskUsageResult['root'];
      unreadableEntries: number;
      unreadable: NonNullable<ScanDiskUsageResult['unreadable']>;
      scannedEntries: number;
      isComplete: boolean;
    }
  | {
      type: 'diskUsage.failed';
      scanId: string;
      code: string;
      message: string;
    }
  | {
      type: 'diskUsage.finalizing';
      scanId: string;
      scannedEntries: number;
    }
  | { type: 'connection.created'; connectionId: ConnectionId }
  | { type: 'connection.updated'; connectionId: ConnectionId }
  | { type: 'connection.statusChanged'; connectionId: ConnectionId; status: ConnectionStatus }
  | { type: 'connection.deleted'; connectionId: ConnectionId };

/** A backend event delivered identically over SSE and Tauri. */
export type BackendEvent = EventEnvelope<BackendEventPayload>;

/** Stops a {@link import('../api/client/file-manager-client').FileManagerClient.subscribe} listener from receiving further events. */
export type Unsubscribe = () => void;
