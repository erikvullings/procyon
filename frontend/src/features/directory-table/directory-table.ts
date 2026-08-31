import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { eyeOffIcon, linkIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  EntryId,
  EntrySummary,
  GitFileStatus,
  LoadingState,
  SortDescriptor,
} from '../../models';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  type EntryFormatSettings,
  formatEntryModifiedAt,
  formatEntrySize,
} from '../entry-formatting/entry-formatting';
import { isParentEntry } from '../panes/parent-entry';
import { fileAgeColumn } from '../plugin-columns/file-age-column';
import { entryIcon } from './entry-icons';
import { finderTagColorSwatch } from './finder-tag-colors';
import type { FinderTagsLoader } from './finder-tags-loader';
import type { NativeIconLoader } from './native-icon-loader';
import type { ThumbnailLoader } from './thumbnail-loader';
import { calculateVisibleWindow, scrollOffsetForIndex } from './windowing';
import './directory-table.css';

const DEFAULT_ROW_HEIGHT = 20;
const DEFAULT_VIEWPORT_HEIGHT = 300;
const DEFAULT_OVERSCAN = 1;
const MIN_COLUMN_WIDTH = 60;

/** A PDF's first-page render, or a video/comic's first frame/page, reads as a nice preview at
 * grid-tile size but not at this 16px list-row size - fetching/decoding one here is wasted work
 * for a result the user won't be able to make out. Fall through to the native/generic icon
 * instead; grid view is unaffected. Plain images (including svg) stay thumbnailed even this
 * small, since a tiny real image is still a legible preview of itself. */
const LIST_VIEW_THUMBNAIL_SKIP_EXTENSIONS = new Set(['pdf', 'mp4', 'm4v', 'mov', 'cbz', 'cbr']);

/** A single column's persisted width, keyed by column id. */
export interface ColumnWidthEntry {
  readonly columnId: string;
  readonly width: number;
}

/** Random-access entry collection; large mock sources need not materialize an array. */
export interface DirectoryEntrySource {
  readonly length: number;
  /** Number of entries currently available without fetching another page. */
  readonly loadedLength?: number;
  entryAt(index: number): EntrySummary | undefined;
}

/** Adapts an ordinary directory snapshot to the random-access table surface.
 * `totalCount`, when larger than `entries.length`, lets the scrollbar/virtualized
 * content height reflect the directory's real size before every page has loaded. */
export function entryArraySource(
  entries: readonly EntrySummary[],
  totalCount?: number,
): DirectoryEntrySource {
  return {
    length: Math.max(totalCount ?? entries.length, entries.length),
    loadedLength: entries.length,
    entryAt: (index) => entries[index],
  };
}

/** Mouse modifiers held during a row click, for shift/ctrl range and toggle selection. */
export interface CursorClickModifiers {
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
}

/** Rendering inputs. Cursor and selection behavior are owned by tasks 0028/0029. */
export interface DirectoryTableAttrs {
  readonly state: LoadingState;
  readonly source?: DirectoryEntrySource;
  readonly cursorIndex?: number;
  readonly selectedEntryIds?: ReadonlySet<EntryId>;
  readonly cutEntryIds?: ReadonlySet<EntryId>;
  readonly active?: boolean;
  readonly viewportHeight?: number;
  readonly overscan?: number;
  readonly label?: string;
  readonly nameMatchPrefix?: string;
  /** Splits search-result names into compact parent-path and filename columns. */
  readonly showFullPath?: boolean;
  readonly sort?: readonly SortDescriptor[];
  readonly onSortChange?: (sort: readonly SortDescriptor[]) => void;
  readonly formatSettings?: EntryFormatSettings;
  readonly nativeIconLoader?: NativeIconLoader;
  /** Overlays a downscaled preview onto the icon column for supported files (task 0134). */
  readonly thumbnailLoader?: ThumbnailLoader;
  /** Overlays Finder-tag color dots next to the name for supported hosts (task 0136). */
  readonly finderTagsLoader?: FinderTagsLoader;
  /** Enabled declarative plugin columns, already validated by the host. */
  readonly pluginColumns?: readonly DirectoryColumnDescriptor[];
  readonly onCursorChange?: (index: number, modifiers?: CursorClickModifiers) => void;
  readonly onActivate?: (index: number) => void;
  readonly onRetry?: () => void;
  readonly onEndReached?: () => void;
  readonly renamingEntryId?: EntryId;
  readonly renameValue?: string;
  readonly renameError?: string;
  readonly onRenameInput?: (value: string) => void;
  readonly onRenameCancel?: () => void;
  readonly onRenameCommit?: () => void;
  readonly onContextMenu?: (index: number | undefined, x: number, y: number) => void;
  readonly onDragStart?: (index: number, event: DragEvent) => void;
  readonly onDragOver?: (index: number | undefined, event: DragEvent) => boolean;
  readonly onDrop?: (index: number | undefined, event: DragEvent) => void;
  /** Persisted per-column widths; a column with no entry falls back to its default track. */
  readonly columnWidths?: readonly ColumnWidthEntry[];
  readonly onColumnWidthChange?: (columnId: string, width: number) => void;
  /** Restricts non-mandatory columns (everything but `core.name`) to this set; `undefined` shows
   * every column. `core.gitStatus` is controlled by `showGitStatusColumn` instead, regardless of
   * membership here. */
  readonly visibleColumnIds?: ReadonlySet<string>;
  /** Shows the Git-status column; only meaningful for directories inside a git repository, and
   * defaults to hidden even then (some users have no git projects). */
  readonly showGitStatusColumn?: boolean;
}

function readRowHeight(element: HTMLElement): number {
  const configured = Number.parseFloat(
    getComputedStyle(element).getPropertyValue('--fm-row-height'),
  );
  return Number.isFinite(configured) && configured > 0 ? configured : DEFAULT_ROW_HEIGHT;
}

/** The viewport reserves scrollbar space via `scrollbar-gutter: stable` (see
 * directory-table.css), so its content remains narrower than the header above it by the
 * platform's actual scrollbar width. Measuring and republishing that width as a CSS custom
 * property lets the header reserve the same amount via `padding-inline-end`, keeping every
 * column aligned whether or not the list is actually tall enough to need scrolling. */
function syncScrollbarWidth(viewport: HTMLElement): void {
  const width = viewport.offsetWidth - viewport.clientWidth;
  const table = viewport.parentElement;
  if (table === null) return;
  const current = table.style.getPropertyValue('--fm-scrollbar-width');
  const next = `${width}px`;
  if (current !== next) table.style.setProperty('--fm-scrollbar-width', next);
}

function typeLabel(entry: EntrySummary): string {
  if (entry.kind === 'directory' || entry.kind === 'symlink') {
    return '';
  }
  return entry.extension ?? entry.mimeType ?? t('table', 'file');
}

function displayName(entry: EntrySummary, separateExtension: boolean): string {
  if (!separateExtension || entry.kind !== 'file') return entry.name;
  const extension = entry.extension;
  if (extension === undefined || extension.length === 0) return entry.name;
  const suffix = `.${extension}`;
  if (
    entry.name.length <= suffix.length ||
    !entry.name.toLocaleLowerCase().endsWith(suffix.toLocaleLowerCase())
  ) {
    return entry.name;
  }
  return entry.name.slice(0, -suffix.length);
}

function rowId(entryId: EntryId): string {
  let hash = 2_166_136_261;
  for (const character of entryId) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16_777_619);
  }
  return `fm-directory-row-${(hash >>> 0).toString(36)}`;
}

export interface DirectoryColumnDescriptor {
  readonly id: string;
  readonly label: string;
  readonly cellClass: string;
  /** Overrides the shared `MIN_COLUMN_WIDTH` clamp for narrow columns (e.g. a one-letter badge). */
  readonly minWidth?: number;
  render(
    entry: EntrySummary,
    nameMatchPrefix?: string,
    formatSettings?: EntryFormatSettings,
    now?: number,
    nativeIconLoader?: NativeIconLoader,
    showFullPath?: boolean,
    thumbnailLoader?: ThumbnailLoader,
    finderTagsLoader?: FinderTagsLoader,
    previousEntry?: EntrySummary,
    separateExtension?: boolean,
  ): m.Children;
}

function parentPath(entry: EntrySummary): string {
  try {
    const url = new URL(entry.location.uri);
    const path = decodeURIComponent(url.pathname);
    const separator = path.replace(/\/+$/u, '').lastIndexOf('/');
    return separator <= 0 ? '/' : path.slice(0, separator);
  } catch {
    return entry.location.uri;
  }
}

function displayPathParts(path: string): { prefix: string; segments: readonly string[] } {
  const homeRelative = path.replace(/^\/Users\/[^/]+(?=\/|$)/u, '~');
  if (homeRelative === '~') return { prefix: '~', segments: [] };
  if (homeRelative.startsWith('~/')) {
    return { prefix: '~', segments: homeRelative.slice(2).split('/') };
  }
  if (homeRelative.startsWith('/')) {
    return { prefix: '/', segments: homeRelative.slice(1).split('/').filter(Boolean) };
  }
  return { prefix: '', segments: homeRelative.split('/').filter(Boolean) };
}

interface CompactPathParts {
  readonly prefix: string;
  readonly collapsedSegments: number;
  readonly segments: readonly string[];
}

function pathGroupRoot(path: string): string {
  const homeGroup = path.match(/^\/Users\/[^/]+(?:\/[^/]+)?/u)?.[0];
  if (homeGroup !== undefined) return homeGroup;
  const volumeGroup = path.match(/^\/Volumes\/[^/]+/u)?.[0];
  if (volumeGroup !== undefined) return volumeGroup;
  return path.match(/^\/[^/]+/u)?.[0] ?? path;
}

function compactParentPath(entry: EntrySummary, previousEntry?: EntrySummary): CompactPathParts {
  const currentPath = parentPath(entry);
  const current = displayPathParts(currentPath);
  if (
    previousEntry === undefined ||
    pathGroupRoot(parentPath(previousEntry)) !== pathGroupRoot(currentPath)
  ) {
    return { ...current, collapsedSegments: 0 };
  }

  const previous = displayPathParts(parentPath(previousEntry));
  let collapsedSegments = 0;
  while (
    collapsedSegments < current.segments.length &&
    current.segments[collapsedSegments] === previous.segments[collapsedSegments]
  ) {
    collapsedSegments += 1;
  }
  if (collapsedSegments === 0) return { ...current, collapsedSegments: 0 };
  return {
    prefix: '',
    collapsedSegments,
    segments: current.segments.slice(collapsedSegments),
  };
}

const GIT_STATUS_LETTERS: Record<GitFileStatus, string> = {
  clean: '',
  modified: 'M',
  staged: 'S',
  untracked: 'U',
  ignored: 'I',
};

const GIT_STATUS_LABELS: Record<GitFileStatus, string> = {
  clean: 'Clean',
  modified: 'Modified',
  staged: 'Staged',
  untracked: 'Untracked',
  ignored: 'Ignored',
};

const INITIAL_COLUMNS: readonly DirectoryColumnDescriptor[] = [
  {
    id: 'core.name',
    label: t('table', 'name'),
    cellClass: 'fm-directory-name',
    render: (
      entry,
      nameMatchPrefix,
      _formatSettings,
      _now,
      nativeIconLoader,
      showFullPath,
      thumbnailLoader,
      finderTagsLoader,
      previousEntry,
      separateExtension = false,
    ) => {
      const searchResult = showFullPath === true && !isParentEntry(entry.id);
      const name = displayName(entry, separateExtension);
      const resultParentPath = searchResult ? parentPath(entry) : undefined;
      const displayedParentPath = searchResult
        ? compactParentPath(entry, previousEntry)
        : undefined;
      const finderTagColors = (finderTagsLoader?.finderTags(entry)?.tags ?? [])
        .map((tag) => finderTagColorSwatch(tag.color))
        .filter((color) => color !== undefined);
      const matchIndex =
        nameMatchPrefix === undefined
          ? -1
          : name.toLocaleLowerCase().indexOf(nameMatchPrefix.toLocaleLowerCase());
      const skipThumbnail = LIST_VIEW_THUMBNAIL_SKIP_EXTENSIONS.has(
        (entry.extension ?? '').toLocaleLowerCase(),
      );
      const thumbnailDataUri = skipThumbnail
        ? undefined
        : thumbnailLoader?.thumbnailDataUri(entry, 'small');
      return [
        thumbnailDataUri !== undefined
          ? m('img.fm-entry-icon.fm-thumbnail-entry-icon', {
              src: thumbnailDataUri,
              width: 16,
              height: 16,
              alt: '',
              'aria-hidden': 'true',
            })
          : nativeIconLoader?.iconDataUri(entry) === undefined
            ? entryIcon(entry, { className: 'fm-entry-icon' })
            : m('img.fm-entry-icon.fm-native-entry-icon', {
                src: nativeIconLoader.iconDataUri(entry),
                width: 16,
                height: 16,
                alt: '',
                'aria-hidden': 'true',
              }),
        m(searchResult ? 'span.fm-entry-name.fm-search-result' : 'span.fm-entry-name', [
          searchResult && resultParentPath !== undefined && displayedParentPath !== undefined
            ? m(
                'span.fm-search-result-parent',
                { title: resultParentPath },
                (() => {
                  const { prefix, collapsedSegments, segments } = displayedParentPath;
                  return [
                    prefix.length === 0
                      ? undefined
                      : m('span.fm-search-result-path-prefix', prefix),
                    ...Array.from({ length: collapsedSegments }, (_, index) => [
                      index === 0 ? undefined : ' ',
                      m('span.fm-search-result-path-collapse', { 'aria-hidden': 'true' }, '.'),
                    ]),
                    ...segments.flatMap((segment, index) => [
                      collapsedSegments > 0 && index === 0
                        ? m('span.fm-search-result-path-separator', { 'aria-hidden': 'true' }, ' /')
                        : prefix === '~' || index > 0
                          ? m(
                              'span.fm-search-result-path-separator',
                              { 'aria-hidden': 'true' },
                              '/',
                            )
                          : undefined,
                      m('span.fm-search-result-path-part', segment),
                    ]),
                  ];
                })(),
              )
            : undefined,
          m(searchResult ? 'span.fm-search-result-name' : 'span', { title: entry.name }, [
            matchIndex < 0 || nameMatchPrefix === undefined
              ? name
              : [
                  name.slice(0, matchIndex),
                  m(
                    'span.fm-typeahead-match',
                    name.slice(matchIndex, matchIndex + nameMatchPrefix.length),
                  ),
                  name.slice(matchIndex + nameMatchPrefix.length),
                ],
          ]),
        ]),
        entry.kind === 'symlink'
          ? m(
              'span.fm-entry-symlink-indicator',
              { title: t('table', 'linkEntry'), 'aria-label': t('table', 'linkEntry') },
              linkIcon({ size: 14 }),
            )
          : undefined,
        entry.hidden
          ? m(
              'span.fm-entry-hidden-indicator',
              { title: t('table', 'hiddenEntry'), 'aria-label': t('table', 'hiddenEntry') },
              eyeOffIcon({ size: 14 }),
            )
          : undefined,
        finderTagColors.length === 0
          ? undefined
          : m(
              'span.fm-entry-finder-tags',
              { title: t('table', 'tagged'), 'aria-label': t('table', 'tagged') },
              finderTagColors.map((color, index) =>
                m('span.fm-entry-finder-tag-dot', { key: index, style: { background: color } }),
              ),
            ),
      ];
    },
  },
  {
    id: 'core.extension',
    label: t('table', 'ext'),
    cellClass: 'fm-directory-type',
    minWidth: 48,
    render: typeLabel,
  },
  {
    id: 'core.size',
    label: t('table', 'size'),
    cellClass: 'fm-directory-size',
    render: (entry, _nameMatchPrefix, settings = DEFAULT_ENTRY_FORMAT_SETTINGS) =>
      isParentEntry(entry.id) || entry.kind === 'symlink' ? '' : formatEntrySize(entry, settings),
  },
  {
    id: 'core.gitStatus',
    label: t('table', 'gitStatus'),
    cellClass: 'fm-directory-git-status',
    minWidth: 32,
    render: (entry) => {
      if (isParentEntry(entry.id) || entry.gitStatus === undefined) return '';
      const letter = GIT_STATUS_LETTERS[entry.gitStatus];
      return letter === ''
        ? ''
        : tooltip(
            GIT_STATUS_LABELS[entry.gitStatus],
            m(
              `span.fm-directory-git-status-badge.fm-directory-git-status-badge--${entry.gitStatus}`,
              letter,
            ),
          );
    },
  },
  {
    id: 'core.modified',
    label: t('table', 'modified'),
    cellClass: 'fm-directory-modified',
    render: (entry, _nameMatchPrefix, settings = DEFAULT_ENTRY_FORMAT_SETTINGS) =>
      isParentEntry(entry.id) ? '' : formatEntryModifiedAt(entry.modifiedAt, settings),
  },
];

/** Safe host-side rendering for the sample plugin's data-only contribution. */
export const SAMPLE_FILE_AGE_COLUMN: DirectoryColumnDescriptor = {
  id: fileAgeColumn.id,
  label: t('table', 'age'),
  cellClass: 'fm-directory-file-age',
  render: (entry, _nameMatchPrefix, _formatSettings, now = Date.now()) =>
    isParentEntry(entry.id) ? '' : fileAgeColumn.display(entry.modifiedAt, now),
};

function stateView(attrs: DirectoryTableAttrs, rowHeight: number): m.Children | undefined {
  if (attrs.state.type === 'loading') {
    if ((attrs.source?.length ?? 0) > 0) {
      return undefined;
    }
    const count = Math.max(
      1,
      Math.ceil((attrs.viewportHeight ?? DEFAULT_VIEWPORT_HEIGHT) / rowHeight),
    );
    return m('.fm-directory-state', { role: 'status', 'aria-live': 'polite' }, [
      m('.fm-visually-hidden', t('table', 'loadingDirectory')),
      Array.from({ length: count }, (_, index) =>
        m('.fm-directory-placeholder', {
          key: index,
          'aria-hidden': 'true',
          style: { height: `${rowHeight}px` },
        }),
      ),
    ]);
  }
  if (attrs.state.type === 'error') {
    const genericMessage = t('table', 'unableToLoad');
    const detail = attrs.state.message.trim();
    const normalizedDetail = detail.replace(/[.!?]+$/, '').toLocaleLowerCase();
    const normalizedGeneric = genericMessage.replace(/[.!?]+$/, '').toLocaleLowerCase();
    return m('.fm-directory-state.fm-directory-error', { role: 'alert' }, [
      m('strong', genericMessage),
      detail.length > 0 && normalizedDetail !== normalizedGeneric ? m('span', detail) : undefined,
      attrs.onRetry === undefined
        ? undefined
        : m(
            'button.fm-directory-retry',
            { type: 'button', onclick: attrs.onRetry },
            t('table', 'retry'),
          ),
    ]);
  }
  if (attrs.state.type === 'idle') {
    return m('.fm-directory-state', { role: 'status' }, t('table', 'notLoaded'));
  }
  if ((attrs.source?.length ?? 0) === 0) {
    return m('.fm-directory-state', { role: 'status' }, t('state', 'empty'));
  }
  return undefined;
}

function nextSort(
  columnId: string,
  sort: readonly SortDescriptor[] | undefined,
): readonly SortDescriptor[] {
  const active = sort?.[0];
  return [
    {
      columnId,
      direction:
        active?.columnId === columnId && active.direction === 'ascending'
          ? 'descending'
          : 'ascending',
    },
  ];
}

function headerView(
  attrs: DirectoryTableAttrs,
  columns: readonly DirectoryColumnDescriptor[],
  widths: ReadonlyMap<string, number> | undefined,
  onResizeStart: (event: PointerEvent, columnId: string, currentWidth: number) => void,
): m.Children {
  return m(
    '.fm-directory-header',
    { role: 'row', style: { gridTemplateColumns: gridTemplate(columns, widths) } },
    columns.map((column) =>
      m(
        `button.fm-directory-cell.${column.cellClass}`,
        {
          key: column.id,
          type: 'button',
          role: 'columnheader',
          'data-column-id': column.id,
          'aria-sort': attrs.sort?.[0]?.columnId === column.id ? attrs.sort[0].direction : 'none',
          onclick: () => attrs.onSortChange?.(nextSort(column.id, attrs.sort)),
          onkeydown: (event: KeyboardEvent) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              attrs.onSortChange?.(nextSort(column.id, attrs.sort));
            }
          },
        },
        [
          column.label,
          attrs.sort?.[0]?.columnId === column.id
            ? m(
                'svg.fm-sort-indicator',
                {
                  'aria-hidden': 'true',
                  viewBox: '0 0 16 16',
                  width: 12,
                  height: 12,
                },
                m('path', {
                  d: attrs.sort[0].direction === 'ascending' ? 'M4 9 8 5l4 4' : 'M4 7l4 4 4-4',
                  fill: 'none',
                  stroke: 'currentColor',
                  'stroke-width': 1.5,
                  'stroke-linecap': 'round',
                  'stroke-linejoin': 'round',
                }),
              )
            : undefined,
          attrs.onColumnWidthChange === undefined
            ? undefined
            : m('span.fm-directory-resize-handle', {
                'aria-hidden': 'true',
                onclick: (event: MouseEvent) => event.stopPropagation(),
                onpointerdown: (event: PointerEvent) => {
                  const button = (event.currentTarget as HTMLElement).closest('button');
                  const currentWidth =
                    widths?.get(column.id) ?? button?.getBoundingClientRect().width ?? 160;
                  // Without capture, a fast/excessive drag that carries the pointer outside the
                  // window loses the `pointermove`/`pointerup` pair entirely - the next drag's own
                  // cleanup then discards the abandoned session with no commit, which looks like
                  // the resize "reverted". Capturing keeps both events targeted at this element
                  // for the rest of the gesture regardless of where the pointer physically is.
                  (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
                  onResizeStart(event, column.id, currentWidth);
                },
              }),
        ],
      ),
    ),
  );
}

function gridTemplate(
  columns: readonly DirectoryColumnDescriptor[],
  widths: ReadonlyMap<string, number> | undefined,
): string {
  // Keyed by column id, not position: `columns` is a *filtered* view of `INITIAL_COLUMNS` (git
  // status is hidden when there's no repo, other core columns are settings-gated, plugin columns
  // are appended). Matching by array index against a fixed-order fallback list meant every column
  // after a hidden one shift left and inherit the WRONG neighbour's fallback track - e.g. with the
  // git-status column hidden, `core.modified` used to land on git-status's tiny
  // `minmax(2.5rem, 0.08fr)` track instead of its own, rendering a few px wide with truncated text.
  const fallbacks: Record<string, string> = {
    'core.name': 'minmax(12rem, 1fr)',
    'core.extension': 'minmax(5rem, 0.2fr)',
    'core.size': 'minmax(6rem, 0.2fr)',
    'core.gitStatus': 'minmax(2.5rem, 0.08fr)',
    'core.modified': 'minmax(10rem, 0.35fr)',
  };
  return columns
    .map((column) => {
      const width = widths?.get(column.id);
      if (width === undefined) return fallbacks[column.id] ?? 'minmax(5rem, 0.2fr)';
      // A stale/legacy persisted width (e.g. from before a column had a resize handle at all)
      // must still be clamped here, not just at drag time - otherwise it renders below the
      // column's minimum forever, with no handle wide enough to grab to fix it.
      return `${Math.max(column.minWidth ?? MIN_COLUMN_WIDTH, width)}px`;
    })
    .join(' ');
}

/**
 * Fixed-row virtualized directory grid. It mounts only the visible window and
 * accepts random-access sources so million-entry fixtures remain lazy.
 */
export const DirectoryTable: FactoryComponent<DirectoryTableAttrs> = () => {
  let element: HTMLElement | undefined;
  let rowHeight = DEFAULT_ROW_HEIGHT;
  let scrollTop = 0;
  let previousCursorIndex: number | undefined;
  // The correct scroll target for a given cursorIndex depends on the entry
  // count too: while `loadAllPages` progressively appends pages, the cursor can
  // jump to (and stay pinned at) the last index before every page has arrived,
  // so the very first sync computes a scrollTop clamped to a small, partial
  // entryCount. Once later pages arrive the entryCount grows but cursorIndex is
  // unchanged, so tracking cursorIndex alone would never re-trigger a resync,
  // leaving the viewport stuck short of the real last entry.
  let previousEntryCount: number | undefined;
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  // When the cursor jumps to an index that requires a large scrollTop while the
  // DOM's scrollable content is still sized for the *previous* render (e.g. right
  // after switching back to a tab whose directory is much longer than whatever
  // tab was showing a moment ago), the browser silently clamps the assignment to
  // fit the stale (smaller) content height. `previousCursorIndex` alone can't
  // detect that: it already recorded the intended index, so nothing would retry
  // the scroll once Mithril patches the content to its real (larger) height.
  // `pendingCursorIndex` tracks that a post-patch recheck is still owed.
  let pendingCursorIndex: number | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let dragTargetIndex: number | undefined;
  /** Live column widths shown while a resize drag is in flight, overriding `attrs.columnWidths`
   * until the persisted value (`sourceColumnWidths`) catches up - mirrors `displayedLayout` in
   * workspace-layout.ts for the split-pane divider. */
  let sourceColumnWidths: readonly ColumnWidthEntry[] | undefined;
  let displayedColumnWidths: readonly ColumnWidthEntry[] | undefined;
  let stopColumnResize: (() => void) | undefined;

  function columnWidthMap(entries: readonly ColumnWidthEntry[] | undefined): Map<string, number> {
    return new Map((entries ?? []).map((entry) => [entry.columnId, entry.width]));
  }

  /** `attrs.columnWidths` is rebuilt with a fresh array (and fresh entry objects) on every render
   * by `pane-content-builder.ts`, even when nothing actually changed - so comparing it against
   * `sourceColumnWidths` by reference (as `workspace-layout.ts` does for `attrs.workspace.layout`,
   * which *is* referentially stable) was true on almost every render, including ones triggered by
   * this component's own `m.redraw()` mid-drag. That stomped `displayedColumnWidths` back to the
   * stale persisted value on the very next render after every `move` handler ran, so a drag never
   * showed any visual feedback (the final width still landed correctly on release, since that path
   * reads the drag's own closure variable rather than `displayedColumnWidths`). Comparing by value
   * instead makes the reconciliation only fire when the persisted width has actually changed. */
  function columnWidthsEqual(
    a: readonly ColumnWidthEntry[] | undefined,
    b: readonly ColumnWidthEntry[] | undefined,
  ): boolean {
    if (a === b) return true;
    if (a === undefined || b === undefined || a.length !== b.length) return false;
    return a.every(
      (entry, index) => entry.columnId === b[index]?.columnId && entry.width === b[index]?.width,
    );
  }

  function beginColumnResize(
    event: PointerEvent,
    attrs: DirectoryTableAttrs,
    columns: readonly DirectoryColumnDescriptor[],
    columnId: string,
    startWidth: number,
  ): void {
    event.preventDefault();
    stopColumnResize?.();
    const startX = event.clientX;
    const minWidth = columns.find((column) => column.id === columnId)?.minWidth ?? MIN_COLUMN_WIDTH;
    let latestWidth = startWidth;
    const move = (moveEvent: PointerEvent): void => {
      // Rounded to a whole pixel: the persisted setting is a `u32` on the backend, and a
      // fractional width (from `getBoundingClientRect()`'s subpixel `startWidth`) fails that
      // validation with a 422, silently discarding the resize before it ever reaches other
      // panes/tabs that share this same global setting.
      latestWidth = Math.round(Math.max(minWidth, startWidth + (moveEvent.clientX - startX)));
      const next = columnWidthMap(displayedColumnWidths ?? attrs.columnWidths);
      next.set(columnId, latestWidth);
      displayedColumnWidths = [...next].map(([id, width]) => ({ columnId: id, width }));
      m.redraw();
    };
    const end = (): void => {
      stopColumnResize?.();
      attrs.onColumnWidthChange?.(columnId, latestWidth);
    };
    stopColumnResize = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      stopColumnResize = undefined;
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
  }

  function applyScrollForCursor(attrs: DirectoryTableAttrs, cursorIndex: number): void {
    if (element === undefined || attrs.source === undefined) return;
    const nextScrollTop = scrollOffsetForIndex({
      index: cursorIndex,
      entryCount: attrs.source.length,
      rowHeight,
      scrollTop: element.scrollTop,
      viewportHeight: attrs.viewportHeight ?? (element.clientHeight || DEFAULT_VIEWPORT_HEIGHT),
    });
    if (nextScrollTop !== element.scrollTop) {
      element.scrollTop = nextScrollTop;
    }
    // Read back the value the browser actually applied rather than assuming the
    // assignment stuck: if the content wasn't tall enough yet, the browser clamps
    // it and `scrollTop` must reflect that reality, not the intended target.
    scrollTop = element.scrollTop;
    pendingCursorIndex = scrollTop === nextScrollTop ? undefined : cursorIndex;
  }

  function syncCursor(attrs: DirectoryTableAttrs): void {
    if (element === undefined || attrs.cursorIndex === undefined || attrs.source === undefined) {
      return;
    }
    const entryCount = attrs.source.length;
    if (attrs.cursorIndex === previousCursorIndex && entryCount === previousEntryCount) {
      return;
    }
    previousCursorIndex = attrs.cursorIndex;
    previousEntryCount = entryCount;
    applyScrollForCursor(attrs, attrs.cursorIndex);
  }

  /** Re-verifies the scroll position once Mithril has patched the DOM with this
   * render's (possibly newly grown) content height, correcting any scrollTop
   * clamped during `syncCursor`'s pre-patch attempt. Returns whether a redraw is
   * needed to re-render the row window at the corrected position. */
  function recheckScroll(attrs: DirectoryTableAttrs): boolean {
    if (pendingCursorIndex === undefined || element === undefined) return false;
    const cursorIndex = pendingCursorIndex;
    const before = scrollTop;
    applyScrollForCursor(attrs, cursorIndex);
    return scrollTop !== before;
  }

  return {
    onremove: () => {
      if (refreshTimer !== undefined) clearInterval(refreshTimer);
      resizeObserver?.disconnect();
      stopColumnResize?.();
    },
    view: ({ attrs }) => {
      syncCursor(attrs);
      if (!columnWidthsEqual(sourceColumnWidths, attrs.columnWidths)) {
        sourceColumnWidths = attrs.columnWidths;
        displayedColumnWidths = sourceColumnWidths;
      }
      const columnWidths = columnWidthMap(displayedColumnWidths);
      const state = stateView(attrs, rowHeight);
      const source = attrs.source;
      const cursorEntry =
        attrs.cursorIndex === undefined ? undefined : source?.entryAt(attrs.cursorIndex);
      const viewportHeight =
        attrs.viewportHeight ?? (element?.clientHeight || DEFAULT_VIEWPORT_HEIGHT);
      const window =
        source === undefined
          ? undefined
          : calculateVisibleWindow({
              entryCount: source.length,
              rowHeight,
              scrollTop,
              viewportHeight,
              overscan: attrs.overscan ?? DEFAULT_OVERSCAN,
            });
      const rows: m.Children[] = [];
      const columns = [...INITIAL_COLUMNS, ...(attrs.pluginColumns ?? [])].filter((column) => {
        if (column.id === 'core.gitStatus') return attrs.showGitStatusColumn === true;
        // Only the fixed built-in columns (besides Name and Git) are settings-gated; plugin
        // columns manage their own visibility upstream.
        if (column.id === 'core.name' || !column.id.startsWith('core.')) return true;
        return attrs.visibleColumnIds === undefined || attrs.visibleColumnIds.has(column.id);
      });
      const separateExtension = columns.some((column) => column.id === 'core.extension');
      const now = Date.now();
      let sawUnloadedEntry = false;
      if (source !== undefined && window !== undefined && state === undefined) {
        for (let index = window.start; index < window.end; index += 1) {
          const entry = source.entryAt(index);
          if (entry === undefined) {
            // Not yet fetched (beyond the loaded pages, ahead of the total known count):
            // request more immediately rather than waiting for the physical scroll bottom,
            // which a fast scroll/jump can reach well before the fetch completes.
            sawUnloadedEntry = true;
            continue;
          }
          const cursor = index === attrs.cursorIndex;
          const selected = attrs.selectedEntryIds?.has(entry.id) ?? false;
          rows.push(
            m(
              '.fm-directory-row',
              {
                key: entry.id,
                id: rowId(entry.id),
                role: 'row',
                'aria-rowindex': index + 2,
                'aria-selected': selected ? 'true' : 'false',
                'data-row-stripe': index % 2 === 1 ? 'alternate' : undefined,
                draggable: attrs.onDragStart === undefined ? undefined : true,
                ondragstart: (event: DragEvent) => attrs.onDragStart?.(index, event),
                ondragover: (event: DragEvent) => {
                  if (attrs.onDragOver?.(index, event) !== true) return;
                  event.preventDefault();
                  dragTargetIndex = index;
                },
                ondragleave: () => {
                  if (dragTargetIndex === index) dragTargetIndex = undefined;
                },
                ondrop: (event: DragEvent) => {
                  event.preventDefault();
                  dragTargetIndex = undefined;
                  attrs.onDrop?.(index, event);
                },
                onclick: (event: MouseEvent) =>
                  attrs.onCursorChange?.(index, {
                    shiftKey: event.shiftKey,
                    ctrlKey: event.ctrlKey || event.metaKey,
                  }),
                oncontextmenu: (event: MouseEvent) => {
                  event.preventDefault();
                  attrs.onContextMenu?.(index, event.clientX, event.clientY);
                },
                ondblclick: () => attrs.onActivate?.(index),
                class: [
                  entry.hidden ? 'fm-hidden-entry' : '',
                  cursor ? 'fm-cursor-row' : '',
                  selected ? 'fm-selected-row' : '',
                  attrs.cutEntryIds?.has(entry.id) === true ? 'fm-cut-entry' : '',
                  dragTargetIndex === index ? 'fm-drop-target' : '',
                ].join(' '),
                style: {
                  height: `${rowHeight}px`,
                  transform: `translateY(${window.offsetTop + (index - window.start) * rowHeight}px)`,
                  gridTemplateColumns: gridTemplate(columns, columnWidths),
                },
              },
              columns.map((column) =>
                m(
                  `.fm-directory-cell.${column.cellClass}`,
                  { key: column.id, role: 'gridcell' },
                  column.id === 'core.name' && attrs.renamingEntryId === entry.id
                    ? [
                        m('input[type=text].fm-inline-rename-input', {
                          value: attrs.renameValue ?? entry.name,
                          'aria-label': t('table', 'rename', { name: entry.name }),
                          'aria-invalid': attrs.renameError === undefined ? undefined : 'true',
                          oncreate: ({ dom }: VnodeDOM) => {
                            const input = dom as HTMLInputElement;
                            input.focus();
                            const dot = entry.kind === 'file' ? entry.name.lastIndexOf('.') : -1;
                            input.setSelectionRange(0, dot > 0 ? dot : entry.name.length);
                          },
                          oninput: (event: InputEvent) =>
                            attrs.onRenameInput?.((event.currentTarget as HTMLInputElement).value),
                          onkeydown: (event: KeyboardEvent) => {
                            if (event.key === 'Escape') {
                              event.preventDefault();
                              event.stopPropagation();
                              attrs.onRenameCancel?.();
                            } else if (event.key === 'Enter') {
                              event.preventDefault();
                              event.stopPropagation();
                              attrs.onRenameCommit?.();
                            }
                          },
                        }),
                        attrs.renameError === undefined
                          ? undefined
                          : m('.fm-inline-rename-error', { role: 'alert' }, attrs.renameError),
                      ]
                    : column.render(
                        entry,
                        attrs.nameMatchPrefix,
                        attrs.formatSettings,
                        now,
                        attrs.nativeIconLoader,
                        attrs.showFullPath,
                        attrs.thumbnailLoader,
                        attrs.finderTagsLoader,
                        index === 0 ? undefined : source.entryAt(index - 1),
                        separateExtension,
                      ),
                ),
              ),
            ),
          );
        }
        // Extend the row stripe pattern into unused viewport space below short directory listings.
        const contentHeight = source.length * rowHeight;
        const fillerCount = Math.max(0, Math.ceil((viewportHeight - contentHeight) / rowHeight));
        for (let i = 0; i < fillerCount; i += 1) {
          const index = source.length + i;
          const fillerTop = contentHeight + i * rowHeight;
          const fillerHeight = Math.min(rowHeight, viewportHeight - fillerTop);
          rows.push(
            m('.fm-directory-row-filler', {
              key: `filler-${i}`,
              'aria-hidden': 'true',
              'data-row-stripe': index % 2 === 1 ? 'alternate' : undefined,
              oncontextmenu: (event: MouseEvent) => {
                event.preventDefault();
                attrs.onContextMenu?.(undefined, event.clientX, event.clientY);
              },
              style: {
                height: `${fillerHeight}px`,
                transform: `translateY(${fillerTop}px)`,
                gridTemplateColumns: gridTemplate(columns, columnWidths),
              },
            }),
          );
        }
        const prefetchRows = Math.ceil(viewportHeight / rowHeight);
        const approachingUnloadedEntries =
          source.loadedLength !== undefined &&
          source.loadedLength < source.length &&
          window.end + prefetchRows >= source.loadedLength;
        if (sawUnloadedEntry || approachingUnloadedEntries) {
          attrs.onEndReached?.();
        }
      }

      return m(
        '.fm-directory-table',
        { style: { height: attrs.viewportHeight === undefined ? '100%' : `${viewportHeight}px` } },
        [
          headerView(attrs, columns, columnWidths, (event, columnId, startWidth) =>
            beginColumnResize(event, attrs, columns, columnId, startWidth),
          ),
          m(
            '.fm-directory-viewport',
            {
              role: 'grid',
              tabindex: 0,
              'aria-label': attrs.label ?? t('table', 'directoryContents'),
              'aria-rowcount': (source?.length ?? 0) + 1,
              'aria-colcount': columns.length,
              'aria-activedescendant':
                cursorEntry === undefined ? undefined : rowId(cursorEntry.id),
              'aria-busy': attrs.state.type === 'loading' ? 'true' : undefined,
              'data-active': attrs.active ? 'true' : 'false',
              oncreate: (vnode: VnodeDOM) => {
                const viewport = vnode.dom as HTMLElement;
                element = viewport;
                rowHeight = readRowHeight(viewport);
                syncScrollbarWidth(viewport);
                if (
                  attrs.pluginColumns?.some((column) => column.id === fileAgeColumn.id) === true
                ) {
                  refreshTimer = setInterval(() => m.redraw(), fileAgeColumn.refreshIntervalMs);
                }
                // Neither a window resize nor a split-pane divider drag triggers a Mithril
                // redraw on its own, so the row window (sized off `element.clientHeight`)
                // would otherwise only catch up once something unrelated redraws.
                if (attrs.viewportHeight === undefined && typeof ResizeObserver !== 'undefined') {
                  resizeObserver = new ResizeObserver(() => {
                    syncScrollbarWidth(viewport);
                    m.redraw();
                  });
                  resizeObserver.observe(viewport);
                }
                syncCursor(attrs);
                m.redraw();
              },
              onupdate: (vnode: VnodeDOM) => {
                element = vnode.dom as HTMLElement;
                syncScrollbarWidth(element);
                const nextRowHeight = readRowHeight(element);
                const rowHeightChanged = nextRowHeight !== rowHeight;
                rowHeight = nextRowHeight;
                const heightChangedAfterLayout =
                  attrs.viewportHeight === undefined && element.clientHeight !== viewportHeight;
                if (
                  rowHeightChanged ||
                  heightChangedAfterLayout ||
                  recheckScroll(vnode.attrs as DirectoryTableAttrs)
                ) {
                  m.redraw();
                }
              },
              onscroll: (event: Event) => {
                const target = event.currentTarget as HTMLElement;
                scrollTop = target.scrollTop;
                if (target.scrollTop + target.clientHeight >= target.scrollHeight - rowHeight) {
                  attrs.onEndReached?.();
                }
              },
              ondragover: (event: DragEvent) => {
                if (
                  event.target instanceof Element &&
                  event.target.closest('.fm-directory-row') !== null
                )
                  return;
                if (attrs.onDragOver?.(undefined, event) === true) event.preventDefault();
              },
              ondrop: (event: DragEvent) => {
                if (
                  event.target instanceof Element &&
                  event.target.closest('.fm-directory-row') !== null
                )
                  return;
                event.preventDefault();
                attrs.onDrop?.(undefined, event);
              },
              oncontextmenu: (event: MouseEvent) => {
                if (
                  !(event.target instanceof Element) ||
                  event.target.closest('.fm-directory-row') === null
                ) {
                  event.preventDefault();
                  attrs.onContextMenu?.(undefined, event.clientX, event.clientY);
                }
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
                event.preventDefault();
                const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
                attrs.onContextMenu?.(attrs.cursorIndex, bounds.left + 12, bounds.top + 12);
              },
            },
            [
              state ??
                m(
                  '.fm-directory-body',
                  {
                    role: 'rowgroup',
                    style: { height: `${Math.max(window?.totalHeight ?? 0, viewportHeight)}px` },
                  },
                  rows,
                ),
              m(
                '.fm-visually-hidden',
                {
                  role: 'status',
                  'aria-live': 'polite',
                  'aria-atomic': 'true',
                  style: { top: '0', left: '0' },
                },
                cursorEntry === undefined
                  ? ''
                  : t('table', 'focusedEntry', { name: cursorEntry.name }),
              ),
            ],
          ),
        ],
      );
    },
  };
};
