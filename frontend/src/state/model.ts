import type {
  BackendNotification,
  ClipboardState,
  ContentMatchSummary,
  EntryId,
  EntrySummary,
  Operation,
  OperationId,
  PaneId,
  PluginDescriptor,
  PluginId,
  RuntimeCapabilities,
  TabProjection,
  WorkspaceProjection,
  WorkspaceViewState,
} from '../models';
import type { RuntimeKind } from '../utilities/runtime';

/** Recursively readonly representation used by application-state snapshots. */
export type DeepReadonly<T> = T extends (...args: never[]) => unknown
  ? T
  : T extends readonly (infer Item)[]
    ? readonly DeepReadonly<Item>[]
    : T extends object
      ? { readonly [Key in keyof T]: DeepReadonly<T[Key]> }
      : T;

/** Runtime selection and capabilities discovered during bootstrap. */
export interface RuntimeState {
  readonly kind: RuntimeKind;
  readonly capabilities?: DeepReadonly<RuntimeCapabilities>;
}

/** A normalized directory snapshot whose entries are keyed by stable identifiers. */
export interface DirectoryState {
  readonly paneId: PaneId;
  readonly sessionId: string;
  readonly requestId: string;
  readonly revision: number;
  readonly writable: boolean;
  readonly entryIds: readonly EntryId[];
  readonly entriesById: Readonly<Partial<Record<EntryId, DeepReadonly<EntrySummary>>>>;
}

/** Authoritative workspace projection and separately cached directory snapshots. */
export interface WorkspaceState {
  readonly current?: DeepReadonly<WorkspaceProjection>;
  readonly directories: Readonly<Partial<Record<string, DirectoryState>>>;
}

/** File operations keyed by stable operation identifier. */
export interface OperationsState {
  readonly byId: Readonly<Partial<Record<OperationId, DeepReadonly<Operation>>>>;
}

/** Discovered plugins keyed by stable plugin identifier. */
export interface PluginsState {
  readonly byId: Readonly<Partial<Record<PluginId, DeepReadonly<PluginDescriptor>>>>;
}

/** Ordered user-visible notifications. */
export interface NotificationsState {
  readonly items: readonly DeepReadonly<BackendNotification>[];
}

/** Backend event-stream lifecycle. */
export interface ConnectionState {
  readonly status: 'connecting' | 'open' | 'reconnecting' | 'closed';
  readonly lastEventId?: number;
  readonly error?: string;
}

/** Uncommitted quick-filter keystroke text per tab key (`${paneId}:${tabId}`). */
export interface QuickFilterDraftsState {
  readonly byTabKey: Readonly<Partial<Record<string, string>>>;
}

/** Most recently closed tab per pane — depth-1 stack for `core.reopenClosedTab`. */
export interface ClosedTabStacksState {
  readonly byPaneId: Readonly<Partial<Record<PaneId, DeepReadonly<TabProjection>>>>;
}

/** Content-match summaries keyed by entry URI, cached from SSE batches for viewer pre-population. */
export interface ContentMatchesState {
  readonly byEntryUri: Readonly<
    Partial<Record<string, readonly DeepReadonly<ContentMatchSummary>[]>>
  >;
}

/** Complete readonly frontend application snapshot. */
export interface AppState {
  readonly runtime: RuntimeState;
  /** In-application copy/cut references; never the system clipboard contents. */
  readonly clipboard: ClipboardState;
  readonly workspace: WorkspaceState;
  readonly workspaceView: DeepReadonly<WorkspaceViewState> | undefined;
  readonly operations: OperationsState;
  readonly plugins: PluginsState;
  readonly notifications: NotificationsState;
  readonly connection: ConnectionState;
  readonly quickFilterDrafts: QuickFilterDraftsState;
  readonly closedTabStacks: ClosedTabStacksState;
  readonly contentMatches: ContentMatchesState;
}

/** Creates the deterministic state used before backend data is received. */
export function createInitialAppState(kind: RuntimeKind): AppState {
  return {
    runtime: { kind },
    clipboard: { locations: [] },
    workspace: { directories: {} },
    workspaceView: undefined,
    operations: { byId: {} },
    plugins: { byId: {} },
    notifications: { items: [] },
    connection: { status: 'closed' },
    quickFilterDrafts: { byTabKey: {} },
    closedTabStacks: { byPaneId: {} },
    contentMatches: { byEntryUri: {} },
  };
}
