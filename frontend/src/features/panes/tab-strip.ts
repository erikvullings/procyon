import m, { type FactoryComponent } from 'mithril';
import { IconButton } from 'mithril-materialized';
import {
  contentSearchIcon,
  heartIcon,
  plugIcon,
  plusIcon,
  searchIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { PaneId, TabId } from '../../models';

interface ActiveTabDrag {
  readonly paneId: PaneId;
  readonly tabId: TabId;
  readonly startX: number;
  readonly startY: number;
  dragging: boolean;
}

interface TabDragPreview {
  readonly paneId: PaneId;
  readonly tabId?: TabId;
  readonly position?: 'before' | 'after';
}

// Reordering/moving tabs uses pointer events rather than the native HTML5 drag-and-drop API:
// Tauri's window-level native drag handling (`dragDropEnabled`, on by default — needed for
// dragging files from the OS into the app) intercepts and swallows in-page `dragstart`/`drop`
// events on some platforms, silently breaking any in-app HTML5 drag-and-drop including this one
// (see docs/architecture and https://github.com/tauri-apps/tauri/issues/14373). Pointer events are
// unaffected by that interception and behave identically across the mock/HTTP/Tauri hosts.
let activeTabDrag: ActiveTabDrag | undefined;
let dragPreview: TabDragPreview | undefined;
let stopTabDrag: (() => void) | undefined;
let suppressNextTabClick = false;

function updateTabDragPreview(x: number, y: number): void {
  const hit = document.elementFromPoint(x, y);
  const paneElement = hit?.closest('[data-pane-id]');
  const paneId = paneElement?.getAttribute('data-pane-id') ?? undefined;
  if (paneId === undefined) {
    dragPreview = undefined;
    return;
  }
  const tabElement = hit?.closest('.fm-pane-tab');
  if (tabElement instanceof HTMLElement) {
    const tabId = tabElement.getAttribute('data-tab-id') ?? undefined;
    if (tabId === undefined) {
      dragPreview = undefined;
      return;
    }
    const bounds = tabElement.getBoundingClientRect();
    const position: 'before' | 'after' =
      bounds.width > 0 && x >= bounds.left + bounds.width / 2 ? 'after' : 'before';
    dragPreview = { paneId: paneId as PaneId, tabId: tabId as TabId, position };
    return;
  }
  if (hit?.closest('.fm-pane-tabs') != null) {
    dragPreview = { paneId: paneId as PaneId };
    return;
  }
  dragPreview = undefined;
}

function finishTabDrag(onMoveTab: TabStripAttrs['onMoveTab']): void {
  if (activeTabDrag === undefined || dragPreview === undefined) return;
  const { paneId: sourcePaneId, tabId: sourceTabId } = activeTabDrag;
  const targetPaneId = dragPreview.paneId;
  const tabElements = [
    ...document.querySelectorAll(`[data-pane-id="${targetPaneId}"] [role="tab"]`),
  ];
  let index: number;
  if (dragPreview.tabId !== undefined) {
    const targetTabIndex = tabElements.findIndex(
      (element) => element.getAttribute('data-tab-id') === dragPreview?.tabId,
    );
    if (targetTabIndex < 0) return;
    index = targetTabIndex + (dragPreview.position === 'after' ? 1 : 0);
  } else {
    index = tabElements.length;
  }
  if (targetPaneId === sourcePaneId) {
    const sourceIndex = tabElements.findIndex(
      (element) => element.getAttribute('data-tab-id') === sourceTabId,
    );
    if (sourceIndex >= 0 && sourceIndex < index) index -= 1;
  }
  onMoveTab(sourcePaneId, sourceTabId, targetPaneId, index);
}

/** One entry in a pane's tab strip (spec §37). */
export interface PaneTab {
  readonly id: TabId;
  readonly title: string;
  /** Full path shown as the tab's tooltip. */
  readonly path: string;
  /** Canonical tab location URI used for scheme/provider-specific behaviour. */
  readonly locationUri?: string;
  /** Whether this tab is a `search://` results tab — shown with a search icon instead of a
   * `search:` text prefix in the tab strip (task 0089 follow-up). */
  readonly isSearchTab?: boolean;
  /** Whether this tab belongs to a saved remote connection. */
  readonly isConnectionTab?: boolean;
  readonly searchKind?: 'filename' | 'content';
}

export interface TabStripAttrs {
  readonly paneId: PaneId;
  readonly tabs: readonly PaneTab[];
  readonly activeTabId: TabId;
  readonly onSelectTab: (tabId: TabId) => void;
  readonly onCloseTab: (tabId: TabId) => void;
  readonly onNewTab: () => void;
  readonly onMoveTab: (
    sourcePaneId: PaneId,
    tabId: TabId,
    targetPaneId: PaneId,
    targetIndex: number,
  ) => void;
  readonly onTabDragOver?: ((tabId: TabId, event: DragEvent) => boolean) | undefined;
  readonly onTabDrop?: ((tabId: TabId, event: DragEvent) => void) | undefined;
  /** Whether the favourites menu is currently open (controls aria-expanded). */
  readonly favouritesOpen: boolean;
  readonly onToggleFavourites: () => void;
  /** Whether to render the trailing new-tab/favourites buttons in this strip. Defaults to
   * `true`; panes with their own breadcrumb bar render those buttons there instead. */
  readonly showActions?: boolean;
}

/** Renders the pane tab bar — individual tabs with drag-to-reorder, new-tab and favourites buttons. */
export const TabStrip: FactoryComponent<TabStripAttrs> = () => {
  // File-drop-onto-tab highlight (external entries dragged from a directory table onto a tab to
  // navigate there). This is a distinct feature from tab reordering below and still uses native
  // HTML5 DnD, since its drag source (directory table rows) is unaffected by this fix.
  let dropTargetTabId: TabId | undefined;

  return {
    view: ({ attrs }) => {
      const preview = dragPreview?.paneId === attrs.paneId ? dragPreview : undefined;
      const appendDropTarget = preview !== undefined && preview.tabId === undefined;
      return m(
        '.fm-pane-tabs',
        {
          role: 'tablist',
          'aria-label': t('pane', 'paneTabs'),
          // Self-contained fallback for pointer-drag hit testing (see `updateTabDragPreview`) —
          // an ancestor wrapper (workspace-layout.ts) also sets this for the same pane, so
          // `closest('[data-pane-id]')` resolves correctly whether or not TabStrip is mounted
          // standalone (e.g. in isolated component tests).
          'data-pane-id': attrs.paneId,
          class: appendDropTarget ? 'fm-tab-append-target' : '',
        },
        [
          ...attrs.tabs.map((tab) => {
            const isReorderTarget = preview?.tabId === tab.id;
            return m(
              '.fm-pane-tab',
              {
                key: tab.id,
                role: 'tab',
                tabindex: 0,
                'data-tab-id': tab.id,
                title: tab.path,
                'aria-selected': tab.id === attrs.activeTabId ? 'true' : 'false',
                onclick: (event: MouseEvent) => {
                  event.stopPropagation();
                  if (suppressNextTabClick) {
                    suppressNextTabClick = false;
                    return;
                  }
                  attrs.onSelectTab(tab.id);
                },
                onkeydown: (event: KeyboardEvent) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    attrs.onSelectTab(tab.id);
                  }
                },
                onpointerdown: (event: PointerEvent) => {
                  if (event.button !== 0) return;
                  if ((event.target as HTMLElement).closest('.fm-pane-tab-close') !== null) return;
                  stopTabDrag?.();
                  activeTabDrag = {
                    paneId: attrs.paneId,
                    tabId: tab.id,
                    startX: event.clientX,
                    startY: event.clientY,
                    dragging: false,
                  };
                  const move = (moveEvent: PointerEvent): void => {
                    if (activeTabDrag === undefined) return;
                    if (!activeTabDrag.dragging) {
                      const dx = moveEvent.clientX - activeTabDrag.startX;
                      const dy = moveEvent.clientY - activeTabDrag.startY;
                      if (Math.abs(dx) < 4 && Math.abs(dy) < 4) return;
                      activeTabDrag.dragging = true;
                    }
                    moveEvent.preventDefault();
                    updateTabDragPreview(moveEvent.clientX, moveEvent.clientY);
                    m.redraw();
                  };
                  const end = (): void => {
                    window.removeEventListener('pointermove', move);
                    window.removeEventListener('pointerup', end);
                    window.removeEventListener('pointercancel', end);
                    stopTabDrag = undefined;
                    if (activeTabDrag?.dragging === true) {
                      suppressNextTabClick = true;
                      finishTabDrag(attrs.onMoveTab);
                    }
                    activeTabDrag = undefined;
                    dragPreview = undefined;
                    m.redraw();
                  };
                  stopTabDrag = end;
                  window.addEventListener('pointermove', move);
                  window.addEventListener('pointerup', end);
                  window.addEventListener('pointercancel', end);
                },
                ondragover: (event: DragEvent) => {
                  const accepted = attrs.onTabDragOver?.(tab.id, event) === true;
                  if (accepted) {
                    event.preventDefault();
                    dropTargetTabId = tab.id;
                  }
                },
                ondragleave: () => {
                  if (dropTargetTabId === tab.id) dropTargetTabId = undefined;
                },
                ondrop: (event: DragEvent) => {
                  event.preventDefault();
                  event.stopPropagation();
                  dropTargetTabId = undefined;
                  attrs.onTabDrop?.(tab.id, event);
                },
                class: [
                  isReorderTarget || dropTargetTabId === tab.id ? 'fm-drop-target' : '',
                  isReorderTarget && preview?.position === 'before' ? 'fm-tab-drop-before' : '',
                  isReorderTarget && preview?.position === 'after' ? 'fm-tab-drop-after' : '',
                  attrs.tabs.length === 1 ? 'fm-pane-tab-only' : '',
                ]
                  .filter((name) => name !== '')
                  .join(' '),
              },
              [
                m(
                  'span.fm-pane-tab-title',
                  tab.isSearchTab === true
                    ? [
                        tab.searchKind === 'content'
                          ? contentSearchIcon({
                              size: 12,
                              className: 'fm-pane-tab-content-search-icon',
                            })
                          : searchIcon({
                              size: 12,
                              className: 'fm-pane-tab-filename-search-icon',
                            }),
                        tab.title,
                      ]
                    : tab.isConnectionTab === true
                      ? [
                          plugIcon({
                            size: 12,
                            className: 'fm-pane-tab-connection-icon',
                          }),
                          tab.title,
                        ]
                      : tab.title,
                ),
                // A pane's only tab has nothing meaningful to close into (spec §37's "leave the
                // pane empty" confirmation exists for the keyboard/menu close path, which has no
                // equivalent "don't even offer it" option) - omitting the button here rather than
                // disabling it means there is no affordance to trigger that confirmation from at
                // all.
                attrs.tabs.length === 1
                  ? undefined
                  : m(
                      'button.fm-pane-tab-close',
                      {
                        type: 'button',
                        'aria-label': t('pane', 'closeNamedTab', { name: tab.title }),
                        tabindex: -1,
                        onclick: (event: MouseEvent) => {
                          event.stopPropagation();
                          attrs.onCloseTab(tab.id);
                        },
                      },
                      '×',
                    ),
              ],
            );
          }),
          ...(attrs.showActions === false
            ? []
            : [
                tooltip(
                  t('pane', 'newTab'),
                  m(
                    IconButton,
                    {
                      className: 'fm-pane-tab-new',
                      'aria-label': t('pane', 'newTab'),
                      onclick: () => attrs.onNewTab(),
                    },
                    plusIcon(),
                  ),
                  { key: '__new-tab__' },
                ),
                tooltip(
                  t('pane', 'favourites'),
                  m(
                    IconButton,
                    {
                      className: 'fm-pane-tab-favourites',
                      'aria-label': t('pane', 'favourites'),
                      'aria-expanded': String(attrs.favouritesOpen),
                      onclick: () => attrs.onToggleFavourites(),
                    },
                    heartIcon(),
                  ),
                  { key: '__favourites__' },
                ),
              ]),
        ],
      );
    },
  };
};
