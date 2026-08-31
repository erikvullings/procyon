import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { t } from '../../i18n';
import type { EntryId, EntrySummary, LoadingState } from '../../models';
import type { CursorClickModifiers, DirectoryEntrySource } from './directory-table';
import { entryIcon } from './entry-icons';
import type { NativeIconLoader } from './native-icon-loader';
import { type GridLine, layoutPhotoLines } from './photo-grouping';
import type { ThumbnailLoader, ThumbnailSize, ThumbnailViewport } from './thumbnail-loader';
import { calculateVisibleWindow } from './windowing';
import './directory-grid.css';

const DEFAULT_TILE_WIDTH = 128;
const DEFAULT_TILE_HEIGHT = 148;
const DEFAULT_VIEWPORT_HEIGHT = 300;
const DEFAULT_VIEWPORT_WIDTH = 300;
const DEFAULT_OVERSCAN = 1;
const DAY_HEADER_HEIGHT = 40;

/** Icon/grid-view mode a pane can be switched to (task 0134). Reuses
 * {@link ThumbnailSize} directly since a tile's on-disk thumbnail size and
 * its on-screen tile size are the same three steps. */
export type GridIconSize = ThumbnailSize;

/** Rendering inputs, mirroring `DirectoryTableAttrs` (task 0024) closely
 * enough that a pane can swap between the two views with the same callback
 * wiring. Selection/cursor semantics, entry source and loading states are
 * shared with the table; only the layout (wrapping tiles instead of fixed
 * rows) differs. */
export interface DirectoryGridAttrs {
  readonly state: LoadingState;
  readonly source?: DirectoryEntrySource;
  readonly cursorIndex?: number;
  readonly selectedEntryIds?: ReadonlySet<EntryId>;
  readonly cutEntryIds?: ReadonlySet<EntryId>;
  readonly viewportHeight?: number;
  readonly overscan?: number;
  readonly label?: string;
  readonly iconSize?: GridIconSize;
  /** "Photo app mode" (task 0134): groups tiles into day sections with a header, derived from
   * each entry's `modifiedAt`. Only the entries already loaded into `source` can be grouped -
   * exactly like the plain grid, an unloaded tail triggers `onEndReached` rather than guessing. */
  readonly photoMode?: boolean;
  readonly nativeIconLoader?: NativeIconLoader;
  readonly thumbnailLoader?: ThumbnailLoader;
  readonly onCursorChange?: (index: number, modifiers?: CursorClickModifiers) => void;
  readonly onActivate?: (index: number) => void;
  readonly onRetry?: () => void;
  readonly onEndReached?: () => void;
  readonly onContextMenu?: (index: number | undefined, x: number, y: number) => void;
  readonly onDragStart?: (index: number, event: DragEvent) => void;
  readonly onDragOver?: (index: number | undefined, event: DragEvent) => boolean;
  readonly onDrop?: (index: number | undefined, event: DragEvent) => void;
}

function tileDimensions(element: HTMLElement | undefined): { width: number; height: number } {
  const style = element === undefined ? undefined : getComputedStyle(element);
  const width = Number.parseFloat(style?.getPropertyValue('--fm-grid-tile-width') ?? '');
  const height = Number.parseFloat(style?.getPropertyValue('--fm-grid-tile-height') ?? '');
  return {
    width: Number.isFinite(width) && width > 0 ? width : DEFAULT_TILE_WIDTH,
    height: Number.isFinite(height) && height > 0 ? height : DEFAULT_TILE_HEIGHT,
  };
}

function stateView(attrs: DirectoryGridAttrs): m.Children | undefined {
  if (attrs.state.type === 'loading') {
    if ((attrs.source?.length ?? 0) > 0) return undefined;
    return m(
      '.fm-directory-state',
      { role: 'status', 'aria-live': 'polite' },
      t('table', 'loadingDirectory'),
    );
  }
  if (attrs.state.type === 'error') {
    const genericMessage = t('table', 'unableToLoad');
    const detail = attrs.state.message.trim();
    return m('.fm-directory-state.fm-directory-error', { role: 'alert' }, [
      m('strong', genericMessage),
      detail.length > 0 ? m('span', detail) : undefined,
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
    return m('.fm-directory-state', { role: 'status' }, t('pane', 'emptyDirectory'));
  }
  return undefined;
}

/** Virtualized thumbnail grid: rows of tiles, windowed the same way the
 * directory table windows fixed rows (task 0134) - a "row" here is a
 * horizontal band of tiles rather than one entry, computed from how many
 * tiles fit across the measured viewport width. */
export const DirectoryGrid: FactoryComponent<DirectoryGridAttrs> = () => {
  let element: HTMLElement | undefined;
  let scrollTop = 0;
  let thumbnailLoader: ThumbnailLoader | undefined;
  let thumbnailViewport: ThumbnailViewport | undefined;

  function renderTile(
    attrs: DirectoryGridAttrs,
    size: GridIconSize,
    tileWidth: number,
    tileHeight: number,
    index: number,
    entry: EntrySummary,
    x: number,
    y: number,
  ): m.Children {
    const cursor = index === attrs.cursorIndex;
    const selected = attrs.selectedEntryIds?.has(entry.id) ?? false;
    const thumbnailDataUri = thumbnailViewport?.thumbnailDataUri(entry, size);
    const nativeIconDataUri = attrs.nativeIconLoader?.iconDataUri(entry);
    return m(
      '.fm-grid-tile',
      {
        key: entry.id,
        role: 'gridcell',
        'aria-selected': selected ? 'true' : 'false',
        draggable: attrs.onDragStart === undefined ? undefined : true,
        ondragstart: (event: DragEvent) => attrs.onDragStart?.(index, event),
        ondragover: (event: DragEvent) => {
          if (attrs.onDragOver?.(index, event) !== true) return;
          event.preventDefault();
        },
        ondrop: (event: DragEvent) => {
          event.preventDefault();
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
          cursor ? 'fm-cursor-tile' : '',
          selected ? 'fm-selected-tile' : '',
          attrs.cutEntryIds?.has(entry.id) === true ? 'fm-cut-entry' : '',
        ].join(' '),
        style: {
          transform: `translate(${x}px, ${y}px)`,
          width: `${tileWidth}px`,
          height: `${tileHeight}px`,
        },
      },
      [
        thumbnailDataUri !== undefined
          ? m('img.fm-grid-thumbnail', {
              src: thumbnailDataUri,
              alt: '',
              'aria-hidden': 'true',
            })
          : nativeIconDataUri === undefined
            ? entryIcon(entry, { className: 'fm-grid-icon', size: 32 })
            : m('img.fm-grid-icon.fm-native-grid-icon', {
                src: nativeIconDataUri,
                alt: '',
                'aria-hidden': 'true',
              }),
        m('span.fm-grid-tile-name', entry.name),
      ],
    );
  }

  return {
    onremove: () => thumbnailViewport?.dispose(),
    view: ({ attrs }) => {
      if (attrs.thumbnailLoader !== thumbnailLoader) {
        thumbnailViewport?.dispose();
        thumbnailLoader = attrs.thumbnailLoader;
        thumbnailViewport = thumbnailLoader?.createViewport();
      }
      thumbnailViewport?.beginFrame();
      const state = stateView(attrs);
      const source = attrs.source;
      const size: GridIconSize = attrs.iconSize ?? 'medium';
      const { width: tileWidth, height: tileHeight } = tileDimensions(element);
      const viewportHeight =
        attrs.viewportHeight ?? (element?.clientHeight || DEFAULT_VIEWPORT_HEIGHT);
      const viewportWidth = element?.clientWidth || DEFAULT_VIEWPORT_WIDTH;
      const columnsPerRow = Math.max(1, Math.floor(viewportWidth / tileWidth));
      const entryCount = source?.length ?? 0;

      const tiles: m.Children[] = [];
      let sawUnloadedEntry = false;
      let contentHeight = 0;

      if (attrs.photoMode === true) {
        const loadedEntries: EntrySummary[] = [];
        if (source !== undefined && state === undefined) {
          for (let index = 0; index < entryCount; index += 1) {
            const entry = source.entryAt(index);
            if (entry === undefined) {
              sawUnloadedEntry = true;
              break;
            }
            loadedEntries.push(entry);
          }
        }
        const lines: GridLine[] = layoutPhotoLines(loadedEntries, columnsPerRow);
        const lineHeights = lines.map((line) =>
          line.kind === 'header' ? DAY_HEADER_HEIGHT : tileHeight,
        );
        const lineOffsets: number[] = [];
        let cursorY = 0;
        for (const height of lineHeights) {
          lineOffsets.push(cursorY);
          cursorY += height;
        }
        contentHeight = cursorY;
        const overscanPx = (attrs.overscan ?? DEFAULT_OVERSCAN) * tileHeight;
        const viewStart = scrollTop - overscanPx;
        const viewEnd = scrollTop + viewportHeight + overscanPx;
        for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
          const line = lines[lineIndex];
          const top = lineOffsets[lineIndex];
          const height = lineHeights[lineIndex];
          if (line === undefined || top === undefined || height === undefined) continue;
          if (top + height < viewStart || top > viewEnd) continue;
          if (line.kind === 'header') {
            tiles.push(
              m(
                '.fm-grid-day-header',
                {
                  key: `day-${lineIndex}`,
                  role: 'rowheader',
                  style: { transform: `translateY(${top}px)`, height: `${height}px` },
                },
                line.label,
              ),
            );
          } else {
            for (let col = 0; col < line.count; col += 1) {
              const index = line.startIndex + col;
              const entry = loadedEntries[index];
              if (entry === undefined) continue;
              tiles.push(
                renderTile(attrs, size, tileWidth, tileHeight, index, entry, col * tileWidth, top),
              );
            }
          }
        }
      } else {
        const rowCount = Math.ceil(entryCount / columnsPerRow);
        const window =
          source === undefined
            ? undefined
            : calculateVisibleWindow({
                entryCount: rowCount,
                rowHeight: tileHeight,
                scrollTop,
                viewportHeight,
                overscan: attrs.overscan ?? DEFAULT_OVERSCAN,
              });
        if (source !== undefined && window !== undefined && state === undefined) {
          const startIndex = window.start * columnsPerRow;
          const endIndex = Math.min(entryCount, window.end * columnsPerRow);
          for (let index = startIndex; index < endIndex; index += 1) {
            const entry = source.entryAt(index);
            if (entry === undefined) {
              sawUnloadedEntry = true;
              continue;
            }
            const row = Math.floor(index / columnsPerRow);
            const col = index % columnsPerRow;
            tiles.push(
              renderTile(
                attrs,
                size,
                tileWidth,
                tileHeight,
                index,
                entry,
                col * tileWidth,
                window.offsetTop + (row - window.start) * tileHeight,
              ),
            );
          }
        }
        contentHeight = window?.totalHeight ?? 0;
      }
      if (sawUnloadedEntry) attrs.onEndReached?.();
      thumbnailViewport?.endFrame();

      return m('.fm-directory-grid', { class: `fm-grid-icon-size-${size}` }, [
        state,
        m(
          '.fm-directory-grid-viewport',
          {
            role: 'grid',
            'aria-label': attrs.label,
            tabindex: -1,
            oncreate: (vnode: VnodeDOM) => {
              element = vnode.dom as HTMLElement;
            },
            onscroll: (event: UIEvent) => {
              const target = event.currentTarget as HTMLElement;
              scrollTop = target.scrollTop;
              const nearBottom =
                target.scrollTop + target.clientHeight >= target.scrollHeight - tileHeight * 2;
              if (nearBottom) attrs.onEndReached?.();
              m.redraw();
            },
          },
          state === undefined
            ? m('.fm-directory-grid-content', { style: { height: `${contentHeight}px` } }, tiles)
            : undefined,
        ),
      ]);
    },
  };
};
