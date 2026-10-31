/**
 * Pure, stateless helpers the application shell (and its wiring modules) share (task 0290).
 *
 * These were `AppShell`'s module-level helpers. They live here because every wiring module needs
 * them and none of them reads shell state: each takes exactly what it needs as arguments.
 */
import m from 'mithril';
import type { Theme } from 'mithril-materialized';
import type { PaneDirectoryView } from '../features/navigation/navigation';
import type { SelectionPlatform } from '../features/selection/keybindings';
import type { SortColumn, SortModel } from '../features/sorting/sorting';
import { t } from '../i18n';
import { usesLiteralControl } from '../keybindings/dispatcher';
import type {
  Location,
  OperationId,
  PaneId,
  ScanDiskUsageResult,
  SortDescriptor,
  SystemLocation,
  TabId,
} from '../models';

/** The shell's initial theme; the settings service is the authority from task 0030 onwards. */
export const DEFAULT_THEME: Theme = 'auto';

/** The sort a pane falls back to when its tab persists none. */
export const DEFAULT_SORT: readonly SortDescriptor[] = [
  { columnId: 'core.name', direction: 'ascending' },
];

/** localStorage key holding operation ids the user dismissed by hand. */
export const DISMISSED_OPERATIONS_STORAGE_KEY = 'fm.dismissedOperationIds';
/** Upper bound on retained dismissed ids, so the list cannot grow without limit. */
export const MAX_DISMISSED_OPERATIONS = 500;

/** Dismissed operation ids restored from localStorage; a malformed entry is dropped. */
export function loadDismissedOperationIds(): Set<OperationId> {
  try {
    const raw = globalThis.localStorage?.getItem(DISMISSED_OPERATIONS_STORAGE_KEY);
    if (raw === null) return new Set();
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((value): value is OperationId => typeof value === 'string'));
  } catch {
    return new Set();
  }
}

/** Persists dismissed operation ids, keeping only the most recent {@link MAX_DISMISSED_OPERATIONS}. */
export function persistDismissedOperationIds(ids: ReadonlySet<OperationId>): void {
  try {
    const recent = [...ids].slice(-MAX_DISMISSED_OPERATIONS);
    globalThis.localStorage?.setItem(DISMISSED_OPERATIONS_STORAGE_KEY, JSON.stringify(recent));
  } catch {
    // localStorage can be unavailable in some runtimes; in-memory dismissal still works.
  }
}

/**
 * Converts a displayed breadcrumb path back to its provider-specific location.
 *
 * `homeDirectory` (the native home-directory path, when known - see `getHomeDirectory` on
 * `FileManagerClient`) lets a leading `~`/`~/...` expand the same way a shell would, for the
 * local provider only: other providers (e.g. SFTP) may have their own, server-side `~`
 * convention keyed to a different home directory, so a bare/unexpandable `~` is passed through
 * unchanged rather than guessing.
 */
export function locationForPath(current: Location, path: string, homeDirectory?: string): Location {
  if (current.providerId === 'archive') {
    const archiveSeparator = path.indexOf('!');
    const outerPath = archiveSeparator < 0 ? path : path.slice(0, archiveSeparator);
    const outerUrl = new URL('file:///');
    outerUrl.pathname = outerPath.replaceAll('\\', '/');
    if (archiveSeparator < 0) {
      return { providerId: 'local', uri: outerUrl.toString() };
    }
    const innerPath = path.slice(archiveSeparator + 1).replace(/^\/+/, '');
    return {
      providerId: 'archive',
      uri: `archive://${outerUrl.toString().slice('file://'.length)}!/${innerPath}`,
    };
  }
  const url = new URL(current.uri);
  const canExpandTilde = current.providerId === 'local' && homeDirectory !== undefined;
  const expandedPath =
    canExpandTilde && path === '~'
      ? homeDirectory
      : canExpandTilde && path.startsWith('~/')
        ? `${homeDirectory.replace(/\/+$/, '')}/${path.slice(2)}`
        : path.startsWith('~')
          ? path
          : path.replaceAll('\\', '/');
  url.pathname = expandedPath.replaceAll('\\', '/');
  return { ...current, uri: url.toString() };
}

/** Removes `sourceUris` from a disk-usage tree, rebasing every ancestor's byte totals. */
export function removeDiskUsageNodes(
  node: ScanDiskUsageResult['root'],
  sourceUris: ReadonlySet<string>,
): ScanDiskUsageResult['root'] | undefined {
  if (sourceUris.has(node.location.uri)) return undefined;
  let changed = false;
  const children = node.children.flatMap((child) => {
    const next = removeDiskUsageNodes(child, sourceUris);
    changed ||= next !== child;
    return next === undefined ? [] : [next];
  });
  if (!changed) return node;
  const previousLogicalBytes = node.children.reduce(
    (total, child) => total + child.logicalBytes,
    0,
  );
  const previousPhysicalBytes = node.children.reduce(
    (total, child) => total + child.physicalBytes,
    0,
  );
  const nextLogicalBytes = children.reduce((total, child) => total + child.logicalBytes, 0);
  const nextPhysicalBytes = children.reduce((total, child) => total + child.physicalBytes, 0);
  return {
    ...node,
    logicalBytes: Math.max(0, node.logicalBytes - previousLogicalBytes + nextLogicalBytes),
    physicalBytes: Math.max(0, node.physicalBytes - previousPhysicalBytes + nextPhysicalBytes),
    children,
  };
}

/** Applies host-detected mount access metadata to a directory view. */
export function respectSystemLocationReadOnly(
  view: PaneDirectoryView,
  locations: readonly SystemLocation[],
): PaneDirectoryView {
  if (
    view.location === undefined ||
    !locations.some(
      ({ location, readOnly }) =>
        readOnly === true &&
        location.providerId === view.location?.providerId &&
        (location.uri === view.location.uri ||
          view.location.uri.startsWith(
            location.uri.endsWith('/') ? location.uri : `${location.uri}/`,
          )),
    )
  ) {
    return view;
  }
  return { ...view, writable: false };
}

/** Windows-style caption-button glyph; text glyphs (−, □, ×) render far smaller than native ones. */
export function windowControlIcon(path: string): m.Vnode {
  return m(
    'svg.fm-windows-titlebar-glyph',
    { viewBox: '0 0 12 12', 'aria-hidden': 'true', focusable: 'false' },
    m('path', { d: path }),
  );
}

/** Composes the per-tab cache key every per-tab runtime cache is keyed by (spec §37). */
export function tabKey(paneId: PaneId, tabId: TabId): string {
  return `${paneId}:${tabId}`;
}

/** The sort a pane uses when its tab persists none. */
export function effectiveSort(sort: readonly SortDescriptor[]): readonly SortDescriptor[] {
  return sort.length === 0 ? DEFAULT_SORT : sort;
}

/** Maps persisted sort descriptors onto the frontend's sortable columns. */
export function frontendSort(sort: readonly SortDescriptor[]): SortModel {
  const descriptor = sort[0];
  if (descriptor === undefined) return [];
  const columns: Readonly<Record<string, SortColumn>> = {
    'core.name': 'name',
    'core.extension': 'extension',
    'core.size': 'size',
    'core.modified': 'modified',
    'sample.fileAge': 'modified',
  };
  const column = columns[descriptor.columnId];
  return column === undefined ? [] : [{ column, direction: descriptor.direction }];
}

/** A human-readable sort description for the status bar. */
export function sortLabel(sort: readonly SortDescriptor[]): string {
  const descriptor = sort[0];
  if (descriptor === undefined) return t('shell', 'unsorted');
  const labels: Readonly<Record<string, string>> = {
    'core.name': t('table', 'name'),
    'core.extension': t('pane', 'extension'),
    'core.size': t('table', 'size'),
    'core.modified': t('table', 'modified'),
    'sample.fileAge': t('table', 'age'),
  };
  return `${labels[descriptor.columnId] ?? descriptor.columnId} ${descriptor.direction}`;
}

/** A short display label for the tree sidebar's root row - the host segment of a remote
 * provider's URI (e.g. `sftp://my-server/` -> "my-server"), or "/" for a local root. */
export function treeRootName(location: Location): string {
  try {
    const host = new URL(location.uri).host;
    return host === '' ? '/' : host;
  } catch {
    return location.providerId;
  }
}

/** The last path segment of a disk-usage root, URI-decoded when it is encoded. */
export function diskUsageRootName(location: Location): string {
  const segment = location.uri.replace(/\/+$/u, '').split('/').at(-1);
  if (segment === undefined || segment === '') return '/';
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/** Renders a keybinding chord for the current platform (`CTRL` -> `Cmd`/`Ctrl`). */
export function displayShortcut(shortcut: string, platform: SelectionPlatform): string {
  const parts = shortcut.split('+');
  const literalControl = usesLiteralControl({ key: parts.at(-1) ?? '', ctrl: true });
  return parts
    .map((part) => {
      if (part === 'CTRL') return platform === 'macos' && !literalControl ? 'Cmd' : 'Ctrl';
      if (part === 'ALT') return platform === 'macos' ? 'Option' : 'Alt';
      if (part === 'SHIFT') return 'Shift';
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join('+');
}

/** A label with its shortcut appended in parentheses, when it has one. */
export function labelWithShortcut(
  label: string,
  shortcut: string | undefined,
  platform: SelectionPlatform,
): string {
  return shortcut === undefined ? label : `${label} (${displayShortcut(shortcut, platform)})`;
}

/** Humanizes a workspace-layer error for a toast, falling back when it is not one. */
export function workspaceErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null) {
    const message = (error as { readonly message?: unknown }).message;
    if (typeof message === 'string' && message.length > 0) return message;
  }
  return fallback;
}
