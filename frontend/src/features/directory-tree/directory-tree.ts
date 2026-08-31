import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { chevronDownIcon, chevronRightIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { calculateVisibleWindow, scrollOffsetForIndex } from '../directory-table/windowing';
import {
  type FlatTreeNode,
  flattenVisibleTree,
  type TreeChildrenState,
} from './directory-tree-state';
import { interpretTreeKey } from './tree-keybindings';
import './directory-tree.css';

const ROW_HEIGHT = 24;
const DEFAULT_OVERSCAN = 8;
const DEFAULT_VIEWPORT_HEIGHT = 240;

export interface DirectoryTreeAttrs {
  /** The node the tree is rooted at — the active pane's provider root. */
  readonly root: { readonly location: Location; readonly name: string };
  readonly state: TreeChildrenState;
  /** The active pane's current location, highlighted and kept in view. */
  readonly activeLocationUri?: string;
  /** Expands a collapsed node (triggering a fetch when its children are not yet cached) or
   * collapses an already-expanded one. */
  readonly onToggleExpand: (location: Location) => void;
  /** Navigates the active pane to `location`. */
  readonly onActivate: (location: Location) => void;
  /** Tab (`direction: 1`) or Shift+Tab (`direction: -1`) pressed while the tree has focus - lets
   * the caller move focus elsewhere (e.g. the next/previous pane) to complete a pane-tree cycle. */
  readonly onTabOut?: (direction: -1 | 1) => void;
  /** Registers a callback the caller can invoke to move DOM focus into the tree - e.g. right
   * after Alt+F10 opens it, so arrow-key navigation works immediately without an extra click
   * (mirrors `TerminalDrawer`'s `registerFocus`). Returns whether focus was actually moved. */
  readonly registerFocus?: (focus: () => boolean) => void;
  readonly viewportHeight?: number;
  readonly overscan?: number;
  readonly label?: string;
}

function rowId(uri: string): string {
  return `fm-tree-row-${encodeURIComponent(uri)}`;
}

/** A keyboard-navigable, lazily-expanding directory-tree sidebar (task 0139). Presentational
 * only: expansion/children caching lives in the caller's `TreeChildrenState` (mirroring how
 * `DirectoryTable` delegates cursor/selection state to `Pane`), so this component only ever
 * renders `attrs.state` and asks for changes via `onToggleExpand`/`onActivate`. */
export const DirectoryTree: FactoryComponent<DirectoryTreeAttrs> = () => {
  let element: HTMLElement | undefined;
  let scrollTop = 0;
  let focusedUri: string | undefined;
  let resizeObserver: ResizeObserver | undefined;

  function clampIndex(index: number, length: number): number {
    return Math.max(0, Math.min(length - 1, index));
  }

  function moveFocusIntoView(
    rows: readonly FlatTreeNode[],
    index: number,
    viewportHeight: number,
  ): void {
    if (element === undefined) return;
    const target = scrollOffsetForIndex({
      entryCount: rows.length,
      rowHeight: ROW_HEIGHT,
      scrollTop,
      viewportHeight,
      index,
    });
    if (target !== scrollTop) {
      scrollTop = target;
      element.scrollTop = target;
    }
  }

  return {
    oninit: ({ attrs }) => {
      attrs.registerFocus?.(() => {
        if (element === undefined) return false;
        element.focus();
        return true;
      });
    },
    view: ({ attrs }) => {
      const rows = flattenVisibleTree(attrs.root, attrs.state);
      const rowsByUri = new Map(rows.map((row, index) => [row.location.uri, index]));
      if (focusedUri === undefined || !rowsByUri.has(focusedUri)) {
        focusedUri =
          attrs.activeLocationUri !== undefined && rowsByUri.has(attrs.activeLocationUri)
            ? attrs.activeLocationUri
            : rows[0]?.location.uri;
      }
      const focusedIndex = focusedUri === undefined ? -1 : (rowsByUri.get(focusedUri) ?? -1);
      const viewportHeight =
        attrs.viewportHeight ?? (element?.clientHeight || DEFAULT_VIEWPORT_HEIGHT);
      const window = calculateVisibleWindow({
        entryCount: rows.length,
        rowHeight: ROW_HEIGHT,
        scrollTop,
        viewportHeight,
        overscan: attrs.overscan ?? DEFAULT_OVERSCAN,
      });

      const children: m.Children[] = [];
      for (let index = window.start; index < window.end; index += 1) {
        const row = rows[index];
        if (row === undefined) continue;
        const selected = row.location.uri === attrs.activeLocationUri;
        const focused = row.location.uri === focusedUri;
        const expandable = row.hasChildren !== false;
        children.push(
          m(
            '.fm-tree-row',
            {
              key: row.location.uri,
              id: rowId(row.location.uri),
              role: 'treeitem',
              'aria-level': row.depth + 1,
              'aria-expanded': expandable ? (row.expanded ? 'true' : 'false') : undefined,
              'aria-selected': selected ? 'true' : 'false',
              class: [selected ? 'fm-tree-row-selected' : '', focused ? 'fm-tree-row-focused' : '']
                .filter(Boolean)
                .join(' '),
              style: {
                transform: `translateY(${window.offsetTop + (index - window.start) * ROW_HEIGHT}px)`,
                paddingLeft: `${row.depth * 16 + 8}px`,
              },
              onclick: () => {
                focusedUri = row.location.uri;
                attrs.onActivate(row.location);
              },
            },
            [
              // The root has nothing to collapse into and no siblings to align with, so it skips
              // the expand-toggle affordance entirely rather than reserving space for a hidden one.
              row.depth === 0
                ? undefined
                : m(
                    'button.fm-tree-expand-toggle',
                    {
                      type: 'button',
                      tabindex: -1,
                      'aria-hidden': 'true',
                      disabled: !expandable,
                      style: { visibility: expandable ? 'visible' : 'hidden' },
                      onclick: (event: MouseEvent) => {
                        event.stopPropagation();
                        attrs.onToggleExpand(row.location);
                      },
                    },
                    row.loading
                      ? m('span.fm-tree-loading-spinner', { 'aria-hidden': 'true' })
                      : row.expanded
                        ? chevronDownIcon({ size: 14 })
                        : chevronRightIcon({ size: 14 }),
                  ),
              m('span.fm-tree-row-name', row.name),
              row.error === undefined
                ? undefined
                : m('span.fm-tree-row-error', { title: row.error, 'aria-hidden': 'true' }, '!'),
            ],
          ),
        );
      }

      return m(
        '.fm-directory-tree',
        {
          role: 'tree',
          tabindex: 0,
          'aria-label': attrs.label ?? t('tree', 'directoryTree'),
          'aria-activedescendant': focusedUri === undefined ? undefined : rowId(focusedUri),
          style: { height: attrs.viewportHeight === undefined ? '100%' : `${viewportHeight}px` },
          oncreate: (vnode: VnodeDOM) => {
            element = vnode.dom as HTMLElement;
            if (attrs.viewportHeight === undefined && typeof ResizeObserver !== 'undefined') {
              resizeObserver = new ResizeObserver(() => m.redraw());
              resizeObserver.observe(element);
            }
            m.redraw();
          },
          onupdate: (vnode: VnodeDOM) => {
            element = vnode.dom as HTMLElement;
          },
          onremove: () => {
            resizeObserver?.disconnect();
          },
          onscroll: (event: Event) => {
            scrollTop = (event.currentTarget as HTMLElement).scrollTop;
          },
          onkeydown: (event: KeyboardEvent) => {
            const row = focusedIndex < 0 ? undefined : rows[focusedIndex];
            if (row === undefined) return;
            const command = interpretTreeKey(event, {
              expanded: row.expanded,
              hasChildren: row.hasChildren,
              depth: row.depth,
            });
            if (command === undefined) return;
            event.preventDefault();
            switch (command.type) {
              case 'moveFocus': {
                const nextIndex = clampIndex(focusedIndex + command.offset, rows.length);
                const next = rows[nextIndex];
                if (next !== undefined) {
                  focusedUri = next.location.uri;
                  moveFocusIntoView(rows, nextIndex, viewportHeight);
                }
                break;
              }
              case 'moveFocusTo': {
                const nextIndex = command.edge === 'first' ? 0 : rows.length - 1;
                const next = rows[nextIndex];
                if (next !== undefined) {
                  focusedUri = next.location.uri;
                  moveFocusIntoView(rows, nextIndex, viewportHeight);
                }
                break;
              }
              case 'expand':
              case 'collapse':
                attrs.onToggleExpand(row.location);
                break;
              case 'moveFocusToFirstChild': {
                const next = rows[focusedIndex + 1];
                if (next !== undefined && next.depth === row.depth + 1) {
                  focusedUri = next.location.uri;
                  moveFocusIntoView(rows, focusedIndex + 1, viewportHeight);
                }
                break;
              }
              case 'moveFocusToParent': {
                for (let index = focusedIndex - 1; index >= 0; index -= 1) {
                  const candidate = rows[index];
                  if (candidate !== undefined && candidate.depth < row.depth) {
                    focusedUri = candidate.location.uri;
                    moveFocusIntoView(rows, index, viewportHeight);
                    break;
                  }
                }
                break;
              }
              case 'activate':
                attrs.onActivate(row.location);
                break;
              case 'moveFocusOut':
                // Handled entirely here rather than relying on the event bubbling to a
                // document-level listener - the tree sits outside any pane, so nothing else is
                // listening for Tab on its behalf.
                event.stopPropagation();
                attrs.onTabOut?.(command.direction);
                break;
            }
          },
        },
        m('.fm-directory-tree-spacer', { style: { height: `${window.totalHeight}px` } }, children),
      );
    },
  };
};
