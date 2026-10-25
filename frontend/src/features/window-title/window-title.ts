import type { Location, PaneId, PaneProjection, WorkspaceProjection } from '../../models';
import type { DeepReadonly } from '../../state/model';

/** Product name shown at the end of every window title. */
export const APP_TITLE = 'Procyon';

/** Maximum total title length; the macOS Dock menu and window switchers truncate long titles. */
export const MAX_WINDOW_TITLE_LENGTH = 60;

const PART_SEPARATOR = ' · ';
const APP_SEPARATOR = ' — ';
const ELLIPSIS = '…';
const MIN_FOLDER_BUDGET = 15;

/** Pure inputs for the window title; every field is derived from the state tree. */
export interface WindowTitleInput {
  readonly workspace: DeepReadonly<WorkspaceProjection> | undefined;
  /** The focused pane; falls back to the workspace's own active pane. */
  readonly focusedPaneId?: PaneId;
}

function ellipsize(text: string, max: number): string {
  if (max <= 0) return '';
  const chars = Array.from(text);
  if (chars.length <= max) return text;
  return `${chars
    .slice(0, Math.max(0, max - 1))
    .join('')
    .trimEnd()}${ELLIPSIS}`;
}

function decode(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

function lastSegment(path: string): string {
  const trimmed = path.replace(/\/+$/u, '');
  return decode(trimmed.slice(trimmed.lastIndexOf('/') + 1));
}

function fileLeaf(path: string): string {
  const leaf = lastSegment(path);
  return leaf.length === 0 ? '/' : leaf;
}

/**
 * Short, human-readable name for a location: the leaf folder name, `/` for a filesystem root,
 * the drive for a drive root, the archive's own name at an archive root, and the provider
 * scheme for the root of a remote connection (whose host segment is an opaque connection id).
 * Returns `undefined` for virtual locations (search results) that have no folder to show.
 */
export function locationLabel(location: Location): string | undefined {
  const uri = location.uri;
  const archive = /^archive:\/\/(.*)$/u.exec(uri);
  if (archive !== null) {
    const rest = archive[1] ?? '';
    const bang = rest.indexOf('!');
    const outer = bang < 0 ? rest : rest.slice(0, bang);
    const inner = bang < 0 ? '' : rest.slice(bang + 1);
    const innerLeaf = lastSegment(inner);
    return innerLeaf.length > 0 ? innerLeaf : fileLeaf(outer);
  }
  if (uri.startsWith('search://')) return undefined;
  const file = /^file:\/\/[^/]*(\/.*)?$/u.exec(uri);
  if (file !== null) return fileLeaf(file[1] ?? '/');
  const remote = /^([a-z][a-z0-9+.-]*):\/\/[^/]*(\/.*)?$/iu.exec(uri);
  if (remote !== null) {
    const leaf = lastSegment(remote[2] ?? '');
    return leaf.length > 0 ? leaf : (remote[1] ?? '').toLowerCase();
  }
  // Bare path without a scheme.
  const bare = lastSegment(uri);
  if (bare.length === 0) return uri.length > 0 ? uri : undefined;
  return bare;
}

function activePane(input: WindowTitleInput): DeepReadonly<PaneProjection> | undefined {
  const workspace = input.workspace;
  if (workspace === undefined) return undefined;
  const wanted = input.focusedPaneId ?? workspace.activePaneId;
  return workspace.panesById[wanted] ?? workspace.panesById[workspace.activePaneId];
}

/**
 * Formats the native window title as `Workspace · folder — Procyon`, most distinguishing parts
 * first because the macOS Dock menu truncates. Blank workspace names and virtual locations are
 * omitted; the total is capped at {@link MAX_WINDOW_TITLE_LENGTH} characters.
 */
export function formatWindowTitle(input: WindowTitleInput): string {
  const suffix = `${APP_SEPARATOR}${APP_TITLE}`;
  const pane = activePane(input);
  const location = pane?.tabsById[pane.activeTabId]?.location;
  let workspaceName = input.workspace?.name.trim() ?? '';
  let folder = location === undefined ? '' : (locationLabel(location) ?? '');

  const budget = MAX_WINDOW_TITLE_LENGTH - Array.from(suffix).length;
  if (workspaceName.length > 0 && folder.length > 0) {
    const sepLength = PART_SEPARATOR.length;
    const wsLength = Array.from(workspaceName).length;
    folder = ellipsize(folder, Math.max(budget - wsLength - sepLength, MIN_FOLDER_BUDGET));
    workspaceName = ellipsize(workspaceName, budget - Array.from(folder).length - sepLength);
  } else {
    workspaceName = ellipsize(workspaceName, budget);
    folder = ellipsize(folder, budget);
  }
  const body = [workspaceName, folder].filter((part) => part.length > 0).join(PART_SEPARATOR);
  return body.length === 0 ? APP_TITLE : `${body}${suffix}`;
}
