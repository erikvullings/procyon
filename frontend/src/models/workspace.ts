import type { CreateWorkspaceRequestDto } from '../api/generated/models/createWorkspaceRequestDto';
import type { DirectoryViewConfigurationDto } from '../api/generated/models/directoryViewConfigurationDto';
import type { OperationCentrePreferencesDto } from '../api/generated/models/operationCentrePreferencesDto';
import type { WorkspaceCommandDto } from '../api/generated/models/workspaceCommandDto';
import type { WorkspaceDto } from '../api/generated/models/workspaceDto';
import type { WorkspaceLayoutDto } from '../api/generated/models/workspaceLayoutDto';
import type { WorkspaceSummaryDto } from '../api/generated/models/workspaceSummaryDto';
import type { EntryId, PaneId, TabId, WorkspaceId } from './ids';
import type { Location } from './location';

/** Persisted directory presentation settings; never contains selection or cursor state. */
export type DirectoryViewConfiguration = DirectoryViewConfigurationDto;

/** Recursive pane layout returned by the workspace backend. */
export type WorkspaceLayout = WorkspaceLayoutDto;

/** Workspace-level operation-centre presentation preferences. */
export type OperationCentrePreferences = OperationCentrePreferencesDto;

/** Lightweight item returned when listing stored workspaces. */
export type WorkspaceSummary = WorkspaceSummaryDto;

/** Input used to create a stored workspace. */
export type CreateWorkspaceRequest = CreateWorkspaceRequestDto;

/** Semantic workspace mutation accepted by every client adapter. */
export type WorkspaceCommand = WorkspaceCommandDto;

/** Normalized tab projection (spec §5.3.13). */
export interface TabProjection {
  id: TabId;
  title: string;
  location: Location;
  canNavigateBack: boolean;
  canNavigateForward: boolean;
  view: DirectoryViewConfiguration;
}

/** Normalized pane projection (spec §5.3.13). */
export interface PaneProjection {
  id: PaneId;
  tabOrder: TabId[];
  tabsById: Record<TabId, TabProjection>;
  activeTabId: TabId;
}

/** Authoritative normalized workspace projection (spec §5.3.13). */
export interface WorkspaceProjection {
  id: WorkspaceId;
  name: string;
  revision: number;
  layout: WorkspaceLayout;
  paneOrder: PaneId[];
  panesById: Record<PaneId, PaneProjection>;
  activePaneId: PaneId;
  operationCentre: OperationCentrePreferences;
  /** True for a per-window fork created for one desktop window's private use, false for a
   * named/template workspace that only changes when explicitly resynced. */
  ephemeral: boolean;
  /** The named workspace this ephemeral workspace was forked from, if any. */
  forkedFrom?: WorkspaceId;
}

/** Frontend-only selection and cursor state for one pane. */
export interface PaneViewState {
  selectedEntryIds: EntryId[];
  cursorEntryId?: EntryId;
}

/** Frontend-only dialog descriptor. Its payload remains owned by the invoking feature. */
export interface DialogState {
  type: string;
}

/** Frontend-only drag state shared by pane views. */
export interface DragState {
  sourceEntryIds: EntryId[];
  targetPaneId?: PaneId;
}

/** Ephemeral UI state kept separate from the serializable projection (spec §5.3.3). */
export interface WorkspaceViewState {
  focusedPaneId: PaneId;
  paneViews: Record<PaneId, PaneViewState>;
  openDialog?: DialogState;
  dragState?: DragState;
}

function titleFromLocation(location: Location): string {
  const withoutTrailingSlashes = location.uri.replace(/\/+$/, '');
  const finalSegment = withoutTrailingSlashes.slice(withoutTrailingSlashes.lastIndexOf('/') + 1);
  if (finalSegment.length === 0) {
    return location.uri;
  }
  // A provider's root (e.g. `file:///`) strips down to nothing but the bare scheme (`file:`)
  // once every trailing slash is gone - show `/` rather than the scheme name.
  if (finalSegment.endsWith(':')) {
    return '/';
  }
  try {
    return decodeURIComponent(finalSegment);
  } catch {
    return finalSegment;
  }
}

function isSessionOnlyLocation(location: Location): boolean {
  return location.uri.startsWith('search://') || location.uri.startsWith('archive://');
}

/** An `archive://` tab's containing folder is derivable from its own URI alone: it always wraps
 * a real `file:` location (before the `!`), regardless of how deep inside the archive the tab was
 * browsing, so its parent directory needs no navigation history to reconstruct. */
function containingFolderForArchive(location: Location): Location {
  const outer = location.uri.slice('archive://'.length).split('!', 1)[0] ?? '';
  try {
    const url = new URL(`file://${outer}`);
    const trimmed = url.pathname.replace(/\/+$/, '');
    const finalSeparator = trimmed.lastIndexOf('/');
    url.pathname = finalSeparator <= 0 ? '/' : trimmed.slice(0, finalSeparator);
    return { providerId: 'local', uri: url.toString() };
  } catch {
    return location;
  }
}

/** A `search://` tab carries no reconstructable location of its own (spec §24: search results are
 * a virtual, session-only location) - its originating folder only survives in the tab's own
 * back-navigation history, most recent last. */
function containingFolderForSearch(history: { back: readonly Location[] }): Location | undefined {
  for (let index = history.back.length - 1; index >= 0; index -= 1) {
    const candidate = history.back[index];
    if (candidate !== undefined && !isSessionOnlyLocation(candidate)) return candidate;
  }
  return undefined;
}

/** `search://` and `archive://` locations are only ever meaningful within the session that opened
 * them (spec §24 for search; archives are never re-mounted from a persisted layout) - reloading a
 * workspace must never try to redisplay or refetch one, so it is swapped for its containing real
 * folder here, once, at hydration time. */
function displayedLocation(tab: {
  location: Location;
  history: { back: readonly Location[] };
}): Location {
  if (tab.location.uri.startsWith('archive://')) return containingFolderForArchive(tab.location);
  if (tab.location.uri.startsWith('search://')) {
    return containingFolderForSearch(tab.history) ?? tab.location;
  }
  return tab.location;
}

/** Converts the persisted DTO into the normalized, directory-free frontend projection.
 *
 * `redirectSessionOnlyTabs` must only be `true` when hydrating a workspace freshly loaded from
 * storage (`getWorkspace`/`openWorkspace`) — every other call site (`dispatchWorkspaceCommand`,
 * `createWorkspace`) reflects a live, in-session state where a `search://`/`archive://` tab is
 * still valid and must keep displaying its own search-icon/breadcrumb header, not be redirected. */
export function workspaceProjectionFromDto(
  workspace: WorkspaceDto,
  { redirectSessionOnlyTabs = false }: { redirectSessionOnlyTabs?: boolean } = {},
): WorkspaceProjection {
  const paneOrder: PaneId[] = [];
  const panesById: Record<PaneId, PaneProjection> = {};

  for (const pane of workspace.panes) {
    paneOrder.push(pane.id);
    const tabOrder: TabId[] = [];
    const tabsById: Record<TabId, TabProjection> = {};
    for (const tab of pane.tabs) {
      tabOrder.push(tab.id);
      const location = redirectSessionOnlyTabs ? displayedLocation(tab) : tab.location;
      const redirected = location !== tab.location;
      tabsById[tab.id] = {
        id: tab.id,
        title: redirected
          ? titleFromLocation(location)
          : (tab.titleOverride ?? titleFromLocation(tab.location)),
        location,
        canNavigateBack: redirected ? false : tab.history.back.length > 0,
        canNavigateForward: redirected ? false : tab.history.forward.length > 0,
        view: tab.view,
      };
    }
    panesById[pane.id] = {
      id: pane.id,
      tabOrder,
      tabsById,
      activeTabId: pane.activeTabId,
    };
  }

  return {
    id: workspace.id,
    name: workspace.name,
    revision: workspace.revision,
    layout: workspace.layout,
    paneOrder,
    panesById,
    activePaneId: workspace.activePaneId,
    operationCentre: workspace.operationCentre,
    ephemeral: workspace.ephemeral ?? false,
    ...(workspace.forkedFrom === undefined || workspace.forkedFrom === null
      ? {}
      : { forkedFrom: workspace.forkedFrom }),
  };
}
