import m, { type FactoryComponent } from 'mithril';
import { placeFloating, visibleBounds } from '../../components/floating-position';
import { arrowBarToRightIcon, layoutSidebarIcon } from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  DiskUsageCleanupCandidate,
  DiskUsageNode,
  Location,
  ScanDiskUsageResult,
} from '../../models';
import {
  buildTreemapScene,
  hitTestTreemap,
  paintTreemap,
  type TreemapScene,
} from './cushion-treemap';
import {
  DEFAULT_TREEMAP_COLOURS,
  nodeColour,
  type Rgb,
  readTreemapPalette,
  type TreemapPalette,
} from './file-type-colours';
import { type TreemapBounds, visibleTreemapChildren } from './treemap-layout';
import './disk-usage-view.css';

export type DiskUsageViewState =
  | { readonly type: 'loading'; readonly rootName: string }
  | { readonly type: 'cancelled'; readonly rootName: string }
  | {
      readonly type: 'loaded';
      readonly result: ScanDiskUsageResult;
      readonly scanning?: boolean;
      readonly finalizing?: boolean;
      readonly error?: string;
    }
  | { readonly type: 'error'; readonly message: string };

export interface DiskUsageViewAttrs {
  readonly state: DiskUsageViewState;
  readonly onOpenFolder: (location: Location) => void;
  readonly onExpandFolder: (location: Location) => void;
  readonly onRetry: () => void;
  readonly onStop: () => void;
  /** Starts a new scan at an ancestor of the scanned folder, from its breadcrumb. */
  readonly onScanFolder?: (location: Location) => void;
  /** Moves a clean-up candidate to the Trash through the confirmed operation flow; resolves
   * `true` once the operation was started. Omitted when the host cannot trash. */
  readonly onTrashFolder?: (location: Location) => Promise<boolean>;
}

const VIEW_BOUNDS: TreemapBounds = { x: 0, y: 0, width: 1000, height: 600 };
const MAX_DEVICE_SCALE = 2;
const MAX_LISTED_ITEMS = 200;

/** Kept across disk-usage views for the session, so hiding the list sticks. */
let listVisible = true;
const LIST_TOGGLE_KEY = 'l';
const LABEL_CSS_FONT_SIZE = 11;

function formatBytes(value: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: unit === 0 ? 0 : 1 }).format(size)} ${units[unit]}`;
}

function formatShare(part: number, whole: number): string {
  return new Intl.NumberFormat(undefined, {
    style: 'percent',
    maximumFractionDigits: 1,
  }).format(whole <= 0 ? 0 : part / whole);
}

function displayLocation(location: Location): string {
  try {
    const url = new URL(location.uri);
    return decodeURIComponent(url.pathname) || location.uri;
  } catch {
    return location.uri;
  }
}

function canDrillInto(node: DiskUsageNode): boolean {
  return node.kind === 'directory' && !node.collapsed && node.children.length > 0;
}

/** Nodes from `root` down to the node with `uri`, or `[root]` when it is no longer present. */
export function diskUsageTrail(root: DiskUsageNode, uri: string | undefined): DiskUsageNode[] {
  if (uri === undefined || root.location.uri === uri) return [root];
  const search = (node: DiskUsageNode): DiskUsageNode[] | undefined => {
    for (const child of node.children) {
      if (child.location.uri === uri) return [child];
      if (child.kind === 'directory' && uri.startsWith(child.location.uri)) {
        const found = search(child);
        if (found !== undefined) return [child, ...found];
      }
    }
    return undefined;
  };
  const found = search(root);
  return found === undefined ? [root] : [root, ...found];
}

/** Breadcrumb targets above a scanned folder, from the filesystem root down to its parent. */
export function diskUsageAncestors(
  location: Location,
): readonly { readonly label: string; readonly location: Location }[] {
  const match = /^([a-z][\w+.-]*:\/\/[^/]*)(\/.*)?$/iu.exec(location.uri);
  if (match === null) return [];
  const prefix = match[1] ?? '';
  const parts = (match[2] ?? '').split('/').filter(Boolean);
  const decode = (part: string) => {
    try {
      return decodeURIComponent(part);
    } catch {
      return part;
    }
  };
  const hasDrive = /^[a-z]:$/iu.test(decode(parts[0] ?? ''));
  const crumbs = parts.map((part, index) => ({
    label: decode(part),
    location: {
      providerId: location.providerId,
      uri: `${prefix}/${parts.slice(0, index + 1).join('/')}`,
    },
  }));
  const withRoot = hasDrive
    ? crumbs
    : [{ label: '/', location: { providerId: location.providerId, uri: `${prefix}/` } }, ...crumbs];
  return withRoot.slice(0, -1);
}

function rgbCss([r, g, b]: Rgb): string {
  return `rgb(${r} ${g} ${b})`;
}

function truncateToWidth(
  context: CanvasRenderingContext2D,
  text: string,
  maximumWidth: number,
): string {
  if (context.measureText(text).width <= maximumWidth) return text;
  let low = 0;
  let high = text.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (context.measureText(`${text.slice(0, middle)}…`).width <= maximumWidth) low = middle;
    else high = middle - 1;
  }
  return low <= 0 ? '' : `${text.slice(0, low)}…`;
}

function paintScene(canvas: HTMLCanvasElement, scene: TreemapScene, palette: TreemapPalette) {
  let context: CanvasRenderingContext2D | null = null;
  try {
    context = canvas.getContext('2d');
  } catch {
    context = null;
  }
  if (context === null || scene.width === 0 || scene.height === 0) return;
  canvas.width = scene.width;
  canvas.height = scene.height;
  context.putImageData(
    new ImageData(paintTreemap(scene, palette.strip), scene.width, scene.height),
    0,
    0,
  );
  const style = getComputedStyle(canvas);
  const fontFamily = style.getPropertyValue('--fm-font-family').trim() || 'sans-serif';
  const labelColour = style.getPropertyValue('--fm-disk-usage-label').trim() || 'white';
  context.font = `600 ${LABEL_CSS_FONT_SIZE * scene.scale}px ${fontFamily}`;
  context.textBaseline = 'middle';
  context.fillStyle = labelColour;
  const padding = 5 * scene.scale;
  for (const label of scene.labels) {
    const x = label.bounds.x * scene.scale;
    const y = label.bounds.y * scene.scale;
    const width = label.bounds.width * scene.scale;
    const height = label.bounds.height * scene.scale;
    const size = formatBytes(label.node.physicalBytes);
    const sizeWidth = context.measureText(size).width;
    const showSize = width > sizeWidth * 3;
    const text = truncateToWidth(
      context,
      label.text,
      width - padding * 2 - (showSize ? sizeWidth + padding : 0),
    );
    context.fillText(text, x + padding, y + height / 2);
    if (showSize) {
      context.globalAlpha = 0.75;
      context.fillText(size, x + width - padding - sizeWidth, y + height / 2);
      context.globalAlpha = 1;
    }
  }
}

export const DiskUsageView: FactoryComponent<DiskUsageViewAttrs> = () => {
  let hovered: DiskUsageNode | undefined;
  let hoverPoint:
    | {
        readonly x: number;
        readonly y: number;
      }
    | undefined;
  let highlight: TreemapBounds | undefined;
  let warningsOpen = false;
  let cleanupOpen = false;
  const trashedUris = new Set<string>();
  const trashingUris = new Set<string>();
  let elapsedSeconds = 0;
  let progressTimer: ReturnType<typeof setInterval> | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let viewBounds = VIEW_BOUNDS;
  let zoomUri: string | undefined;
  let scene: TreemapScene | undefined;
  let sceneKey:
    | {
        readonly node: DiskUsageNode;
        readonly width: number;
        readonly height: number;
        readonly scale: number;
        readonly palette: string;
      }
    | undefined;
  let paintedScene: TreemapScene | undefined;
  let palette: TreemapPalette = DEFAULT_TREEMAP_COLOURS;

  function updateProgressTimer(state: DiskUsageViewState): void {
    const scanning =
      state.type === 'loading' || (state.type === 'loaded' && state.scanning === true);
    if (scanning && progressTimer === undefined) {
      elapsedSeconds = 0;
      progressTimer = setInterval(() => {
        elapsedSeconds += 1;
        m.redraw();
      }, 1_000);
    } else if (!scanning && progressTimer !== undefined) {
      clearInterval(progressTimer);
      progressTimer = undefined;
    }
  }

  function deviceScale(): number {
    const ratio = typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1;
    return Math.min(MAX_DEVICE_SCALE, Math.max(1, ratio));
  }

  /** Rebuilds the layout only when its inputs change; hit-testing reuses the cached scene. */
  function sceneFor(node: DiskUsageNode, palette: TreemapPalette): TreemapScene {
    const scale = deviceScale();
    const paletteKey = JSON.stringify(palette);
    if (
      scene === undefined ||
      sceneKey?.node !== node ||
      sceneKey.width !== viewBounds.width ||
      sceneKey.height !== viewBounds.height ||
      sceneKey.scale !== scale ||
      sceneKey.palette !== paletteKey
    ) {
      scene = buildTreemapScene(node, viewBounds.width, viewBounds.height, scale, palette);
      sceneKey = {
        node,
        width: viewBounds.width,
        height: viewBounds.height,
        scale,
        palette: paletteKey,
      };
    }
    return scene;
  }

  function clearHover(): void {
    hovered = undefined;
    hoverPoint = undefined;
    highlight = undefined;
  }

  function pointFrom(event: MouseEvent, element: Element | null) {
    const view = element?.closest('.fm-disk-usage-view');
    const bounds = view?.getBoundingClientRect();
    return bounds === undefined
      ? undefined
      : {
          x: event.clientX - bounds.left,
          y: event.clientY - bounds.top,
        };
  }

  function placeHoverTooltip(dom: Element): void {
    const view = dom.parentElement;
    if (hoverPoint === undefined || !(dom instanceof HTMLElement) || view === null) return;
    const viewRect = view.getBoundingClientRect();
    const { left, top } = placeFloating(
      { x: viewRect.left + hoverPoint.x, y: viewRect.top + hoverPoint.y },
      { width: dom.offsetWidth, height: dom.offsetHeight },
      visibleBounds(viewRect),
    );
    dom.style.left = `${left - viewRect.left}px`;
    dom.style.top = `${top - viewRect.top}px`;
  }

  function zoomTo(node: DiskUsageNode | undefined): void {
    zoomUri = node?.location.uri;
    clearHover();
  }

  /** Drills into a directory, rescans a collapsed one, or opens a file's folder elsewhere. */
  function activate(node: DiskUsageNode, parent: DiskUsageNode, attrs: DiskUsageViewAttrs): void {
    if (node.kind === 'directory' && node.collapsed) {
      if (attrs.state.type === 'loaded' && attrs.state.scanning !== true) {
        zoomTo(node);
        attrs.onExpandFolder(node.location);
      }
      return;
    }
    if (canDrillInto(node)) {
      zoomTo(node);
      return;
    }
    attrs.onOpenFolder(node.kind === 'directory' ? node.location : parent.location);
  }

  function canTrash(location: Location): boolean {
    return location.providerId === 'local' || location.providerId === 'file';
  }

  function trashCandidate(location: Location, attrs: DiskUsageViewAttrs): void {
    if (attrs.onTrashFolder === undefined || trashingUris.has(location.uri)) return;
    trashingUris.add(location.uri);
    void attrs
      .onTrashFolder(location)
      .then((started) => {
        if (started) trashedUris.add(location.uri);
      })
      .catch((error: unknown) => {
        console.warn('Failed to move disk-usage clean-up candidate to Trash', error);
      })
      .finally(() => {
        trashingUris.delete(location.uri);
        m.redraw();
      });
  }

  function renderCleanup(
    candidates: readonly DiskUsageCleanupCandidate[],
    attrs: DiskUsageViewAttrs,
  ): m.Children {
    return m('.fm-disk-usage-cleanup', [
      m('strong', t('diskUsage', 'cleanupHeading')),
      m('p', t('diskUsage', 'cleanupExplanation')),
      m(
        'ul',
        candidates.map((candidate) =>
          m('li', { key: candidate.location.uri }, [
            m('.fm-disk-usage-cleanup-details', [
              m('span.fm-disk-usage-cleanup-path', displayLocation(candidate.location)),
              m('span.fm-disk-usage-cleanup-kind', t('diskUsage', `cleanupKind_${candidate.kind}`)),
            ]),
            m('span.fm-disk-usage-cleanup-size', formatBytes(candidate.physicalBytes)),
            m(
              'button.btn-flat.fm-disk-usage-cleanup-show',
              { type: 'button', onclick: () => attrs.onOpenFolder(candidate.location) },
              t('diskUsage', 'cleanupShow'),
            ),
            attrs.onTrashFolder === undefined || !canTrash(candidate.location)
              ? undefined
              : m(
                  'button.btn.fm-disk-usage-cleanup-trash',
                  {
                    type: 'button',
                    disabled: trashingUris.has(candidate.location.uri),
                    onclick: () => trashCandidate(candidate.location, attrs),
                  },
                  t('diskUsage', 'cleanupTrash'),
                ),
          ]),
        ),
      ),
    ]);
  }

  function renderHeader(
    root: DiskUsageNode,
    trail: readonly DiskUsageNode[],
    current: DiskUsageNode,
    attrs: DiskUsageViewAttrs,
  ): m.Children {
    const onScanFolder = attrs.onScanFolder;
    const listLabel = `${t('diskUsage', listVisible ? 'hideList' : 'showList')} (${LIST_TOGGLE_KEY.toUpperCase()})`;
    return m('.fm-breadcrumb-row.fm-disk-usage-header', [
      m('nav.fm-breadcrumb', { 'aria-label': t('diskUsage', 'breadcrumbLabel') }, [
        m('.fm-breadcrumb-segments', [
          ...diskUsageAncestors(root.location).map((ancestor) =>
            onScanFolder === undefined || attrs.state.type !== 'loaded' || attrs.state.scanning
              ? m('span.fm-breadcrumb-segment', { key: ancestor.location.uri }, ancestor.label)
              : m(
                  'button.fm-breadcrumb-segment.fm-disk-usage-ancestor',
                  {
                    key: ancestor.location.uri,
                    type: 'button',
                    onclick: () => {
                      zoomTo(undefined);
                      onScanFolder(ancestor.location);
                    },
                  },
                  ancestor.label,
                ),
          ),
          ...trail.map((node, index) =>
            index === trail.length - 1
              ? m(
                  'span.fm-breadcrumb-segment',
                  { key: node.location.uri, 'aria-current': 'location' },
                  node.name,
                )
              : m(
                  'button.fm-breadcrumb-segment.fm-disk-usage-crumb',
                  { key: node.location.uri, type: 'button', onclick: () => zoomTo(node) },
                  node.name,
                ),
          ),
        ]),
      ]),
      tooltip(
        listLabel,
        m(
          'button.btn-flat.btn-icon.fm-disk-usage-header-action.fm-disk-usage-list-toggle',
          {
            type: 'button',
            'aria-label': listLabel,
            'aria-pressed': String(listVisible),
            'aria-keyshortcuts': LIST_TOGGLE_KEY.toUpperCase(),
            onclick: () => {
              listVisible = !listVisible;
            },
          },
          layoutSidebarIcon({ size: 16 }),
        ),
      ),
      tooltip(
        t('diskUsage', 'openInOtherPane'),
        m(
          'button.btn-flat.btn-icon.fm-disk-usage-header-action.fm-disk-usage-open-current',
          {
            type: 'button',
            'aria-label': t('diskUsage', 'openInOtherPane'),
            onclick: () => attrs.onOpenFolder(current.location),
          },
          arrowBarToRightIcon({ size: 16 }),
        ),
      ),
    ]);
  }

  function renderItems(
    current: DiskUsageNode,
    colours: TreemapPalette,
    attrs: DiskUsageViewAttrs,
  ): m.Children {
    const items = visibleTreemapChildren(current.children, current.physicalBytes).slice(
      0,
      MAX_LISTED_ITEMS,
    );
    return m(
      'ul.fm-disk-usage-items',
      { 'aria-label': t('diskUsage', 'itemsLabel', { name: current.name }) },
      items.map((node) => {
        const size = formatBytes(node.physicalBytes);
        const share = current.physicalBytes <= 0 ? 0 : node.physicalBytes / current.physicalBytes;
        const tileBounds = () =>
          scene?.tiles.find((candidate) => candidate.depth === 1 && candidate.node === node)
            ?.bounds;
        return m(
          'li.fm-disk-usage-item',
          {
            key: `${node.location.uri}\u0000${node.name}`,
            class: hovered === node ? 'fm-disk-usage-item--hovered' : undefined,
            onpointerenter: (event: PointerEvent) => {
              hovered = node;
              hoverPoint = pointFrom(event, event.currentTarget as Element | null);
              highlight = tileBounds();
            },
            onpointerleave: clearHover,
          },
          [
            m(
              'button.fm-disk-usage-item-activate',
              {
                type: 'button',
                'aria-label': `${node.name}, ${size}`,
                onclick: () => activate(node, current, attrs),
                onfocus: () => {
                  hovered = node;
                  hoverPoint = undefined;
                  highlight = tileBounds();
                },
                onblur: clearHover,
              },
              [
                m('span.fm-disk-usage-swatch', {
                  style: { background: rgbCss(nodeColour(node, colours)) },
                  'aria-hidden': 'true',
                }),
                m('span.fm-disk-usage-item-name', node.name),
                m('span.fm-disk-usage-item-size', size),
                m(
                  'span.fm-disk-usage-item-share',
                  { 'aria-hidden': 'true' },
                  m('span', { style: { width: `${Math.max(1, share * 100)}%` } }),
                ),
                m(
                  'span.fm-disk-usage-item-percent',
                  formatShare(node.physicalBytes, current.physicalBytes),
                ),
              ],
            ),
            node.kind === 'directory'
              ? tooltip(
                  t('diskUsage', 'openInOtherPane'),
                  m(
                    'button.btn-flat.fm-disk-usage-item-open',
                    {
                      type: 'button',
                      'aria-label': `${t('diskUsage', 'openInOtherPane')}: ${node.name}`,
                      onclick: () => attrs.onOpenFolder(node.location),
                    },
                    arrowBarToRightIcon({ size: 14 }),
                  ),
                )
              : undefined,
          ],
        );
      }),
    );
  }

  function renderMap(
    current: DiskUsageNode,
    trail: readonly DiskUsageNode[],
    attrs: DiskUsageViewAttrs,
  ): m.Children {
    const repaint = (canvas: HTMLCanvasElement) => {
      palette = readTreemapPalette(canvas);
      const next = sceneFor(current, palette);
      if (paintedScene === next) return;
      paintScene(canvas, next, palette);
      paintedScene = next;
    };
    const tileAt = (event: MouseEvent) => {
      const target = event.currentTarget as HTMLCanvasElement | null;
      const bounds = target?.getBoundingClientRect();
      if (scene === undefined || bounds === undefined) return undefined;
      return hitTestTreemap(scene, event.clientX - bounds.left, event.clientY - bounds.top);
    };
    return m(
      '.fm-disk-usage-canvas-host',
      {
        tabindex: 0,
        oncreate: ({ dom }: m.VnodeDOM) => {
          // Take over focus from the surrounding pane so the view's keys (L, Backspace) work.
          const focused = document.activeElement;
          if (focused === document.body || focused === dom.closest('.fm-pane')) {
            (dom as HTMLElement).focus();
          }
          const updateBounds = () => {
            const { width, height } = dom.getBoundingClientRect();
            if (width <= 0 || height <= 0) return;
            if (width === viewBounds.width && height === viewBounds.height) return;
            viewBounds = { x: 0, y: 0, width, height };
            m.redraw();
          };
          updateBounds();
          if (typeof ResizeObserver !== 'undefined') {
            resizeObserver = new ResizeObserver(updateBounds);
            resizeObserver.observe(dom);
          }
        },
      },
      [
        m('canvas.fm-disk-usage-canvas', {
          role: 'img',
          'aria-label': t('diskUsage', 'treemapLabel', { name: current.name }),
          oncreate: ({ dom }: m.VnodeDOM) => repaint(dom as HTMLCanvasElement),
          onupdate: ({ dom }: m.VnodeDOM) => repaint(dom as HTMLCanvasElement),
          onpointermove: (event: PointerEvent) => {
            const tile = tileAt(event);
            if (tile?.node === hovered && hoverPoint !== undefined) {
              hoverPoint = pointFrom(event, event.currentTarget as Element | null);
              return;
            }
            hovered = tile?.node;
            highlight = tile?.bounds;
            hoverPoint =
              tile === undefined
                ? undefined
                : pointFrom(event, event.currentTarget as Element | null);
          },
          onpointerleave: clearHover,
          onclick: (event: MouseEvent) => {
            // The workspace pane's own click handler focuses its section after this one runs;
            // reclaim focus afterwards so Backspace/Escape reach this view's zoom-out handler.
            const host = (event.currentTarget as HTMLElement | null)?.parentElement;
            setTimeout(() => host?.focus(), 0);
            const tile = tileAt(event);
            const target = tile?.trail[0];
            if (target === undefined) return;
            if (target.kind !== 'directory') {
              hovered = tile?.node;
              return;
            }
            activate(target, current, attrs);
          },
        }),
        highlight === undefined
          ? undefined
          : m('.fm-disk-usage-highlight', {
              'aria-hidden': 'true',
              style: {
                left: `${highlight.x}px`,
                top: `${highlight.y}px`,
                width: `${highlight.width}px`,
                height: `${highlight.height}px`,
              },
            }),
        trail.length > 1 ? m('span.fm-visually-hidden', t('diskUsage', 'zoomHint')) : undefined,
      ],
    );
  }

  return {
    onremove: () => {
      if (progressTimer !== undefined) clearInterval(progressTimer);
      resizeObserver?.disconnect();
    },
    view: ({ attrs }) => {
      updateProgressTimer(attrs.state);
      if (attrs.state.type === 'loading') {
        return m('.fm-disk-usage-status', [
          m('.fm-disk-usage-spinner', { 'aria-hidden': 'true' }),
          m('strong', t('diskUsage', 'scanning', { name: attrs.state.rootName })),
          m('span', t('diskUsage', 'elapsed', { seconds: elapsedSeconds })),
          m('button.btn', { type: 'button', onclick: attrs.onStop }, t('diskUsage', 'stop')),
        ]);
      }
      if (attrs.state.type === 'cancelled') {
        return m('.fm-disk-usage-status', [
          m('strong', t('diskUsage', 'stopped', { name: attrs.state.rootName })),
          m('button.btn', { type: 'button', onclick: attrs.onRetry }, t('diskUsage', 'retry')),
        ]);
      }
      if (attrs.state.type === 'error') {
        return m('.fm-disk-usage-status', [
          m('p', attrs.state.message),
          m('button.btn', { type: 'button', onclick: attrs.onRetry }, t('diskUsage', 'retry')),
        ]);
      }
      const { result } = attrs.state;
      const trail = diskUsageTrail(result.root, zoomUri);
      const current = trail.at(-1) ?? result.root;
      const unreadable = result.unreadable ?? [];
      const cleanupCandidates = (result.cleanupCandidates ?? []).filter(
        (candidate) => !trashedUris.has(candidate.location.uri),
      );
      const cleanupBytes = cleanupCandidates.reduce(
        (total, candidate) => total + candidate.physicalBytes,
        0,
      );
      const scannedEntries = result.scannedEntries ?? 0;
      const hasContent = visibleTreemapChildren(current.children, current.physicalBytes).length > 0;
      return m(
        '.fm-disk-usage-view',
        {
          onkeydown: (event: KeyboardEvent) => {
            if (
              event.key.toLowerCase() === LIST_TOGGLE_KEY &&
              !event.ctrlKey &&
              !event.metaKey &&
              !event.altKey &&
              !event.shiftKey
            ) {
              event.preventDefault();
              event.stopPropagation();
              listVisible = !listVisible;
              return;
            }
            if (
              (event.key === 'Backspace' || event.key === 'Escape') &&
              trail.length > 1 &&
              !event.ctrlKey &&
              !event.metaKey &&
              !event.altKey
            ) {
              event.preventDefault();
              event.stopPropagation();
              zoomTo(trail.at(-2));
            }
          },
        },
        [
          renderHeader(result.root, trail, current, attrs),
          !hasContent && trail.length === 1
            ? m('.fm-disk-usage-status', t('diskUsage', 'empty'))
            : m(
                '.fm-disk-usage-body',
                { class: listVisible ? undefined : 'fm-disk-usage-body--map-only' },
                [
                  listVisible ? renderItems(current, palette, attrs) : undefined,
                  renderMap(current, trail, attrs),
                ],
              ),
          attrs.state.error !== undefined ||
          (warningsOpen && unreadable.length > 0) ||
          (cleanupOpen && cleanupCandidates.length > 0)
            ? m('.fm-disk-usage-notices', [
                cleanupOpen && cleanupCandidates.length > 0
                  ? renderCleanup(cleanupCandidates, attrs)
                  : undefined,
                attrs.state.error === undefined
                  ? undefined
                  : m('.fm-disk-usage-failure', { role: 'alert' }, [
                      m('span', attrs.state.error),
                      m(
                        'button.btn',
                        { type: 'button', onclick: attrs.onRetry },
                        t('diskUsage', 'retry'),
                      ),
                    ]),
                warningsOpen && unreadable.length > 0
                  ? m('.fm-disk-usage-warnings', [
                      m('strong', t('diskUsage', 'unreadableHeading')),
                      m('p', t('diskUsage', 'unreadableExplanation')),
                      m(
                        'ul',
                        unreadable.map((entry) =>
                          m('li', [
                            m('span', displayLocation(entry.location)),
                            m(
                              'span.fm-disk-usage-warning-reason',
                              t('diskUsage', `unreadableReason_${entry.reason}`),
                            ),
                          ]),
                        ),
                      ),
                      result.unreadableEntries > unreadable.length
                        ? m(
                            'p',
                            t('diskUsage', 'unreadableMore', {
                              count: result.unreadableEntries - unreadable.length,
                            }),
                          )
                        : undefined,
                    ])
                  : undefined,
              ])
            : undefined,
          m('.fm-pane-status.fm-disk-usage-statusbar', [
            m(
              'span.fm-disk-usage-total',
              { role: 'status' },
              trail.length > 1
                ? t('diskUsage', 'zoomedSize', {
                    size: formatBytes(current.physicalBytes),
                    total: formatBytes(result.root.physicalBytes),
                    share: formatShare(current.physicalBytes, result.root.physicalBytes),
                  })
                : formatBytes(result.root.physicalBytes),
            ),
            attrs.state.scanning === true
              ? m('.fm-disk-usage-progress', [
                  m('.fm-disk-usage-spinner.fm-disk-usage-spinner--compact', {
                    'aria-hidden': 'true',
                  }),
                  m(
                    'span',
                    attrs.state.finalizing === true
                      ? t('diskUsage', 'finalizing', {
                          seconds: elapsedSeconds,
                          count: new Intl.NumberFormat().format(scannedEntries),
                        })
                      : t('diskUsage', 'updating', {
                          seconds: elapsedSeconds,
                          count: new Intl.NumberFormat().format(scannedEntries),
                        }),
                  ),
                  m(
                    'button.btn-flat.fm-disk-usage-status-action.fm-disk-usage-stop',
                    { type: 'button', onclick: attrs.onStop },
                    t('diskUsage', 'stop'),
                  ),
                ])
              : undefined,
            cleanupCandidates.length > 0
              ? m(
                  'button.btn-flat.fm-disk-usage-status-action.fm-disk-usage-cleanup-toggle',
                  {
                    type: 'button',
                    'aria-expanded': String(cleanupOpen),
                    onclick: () => {
                      cleanupOpen = !cleanupOpen;
                    },
                  },
                  t('diskUsage', 'cleanupButton', {
                    count: cleanupCandidates.length,
                    size: formatBytes(cleanupBytes),
                  }),
                )
              : undefined,
            result.unreadableEntries > 0
              ? m(
                  'button.btn-flat.fm-disk-usage-status-action.fm-disk-usage-warning',
                  {
                    type: 'button',
                    'aria-expanded': String(warningsOpen),
                    onclick: () => {
                      warningsOpen = !warningsOpen;
                    },
                  },
                  t('diskUsage', 'unreadable', { count: result.unreadableEntries }),
                )
              : undefined,
          ]),
          hovered !== undefined && hoverPoint !== undefined
            ? m(
                '.fm-disk-usage-tooltip',
                {
                  oncreate: ({ dom }: m.VnodeDOM) => placeHoverTooltip(dom),
                  onupdate: ({ dom }: m.VnodeDOM) => placeHoverTooltip(dom),
                },
                [
                  m('strong', hovered.name),
                  m('span', displayLocation(hovered.location)),
                  m(
                    'span',
                    `${t('diskUsage', 'logical')}: ${formatBytes(hovered.logicalBytes)} · ${t('diskUsage', 'physical')}: ${formatBytes(hovered.physicalBytes)} · ${formatShare(hovered.physicalBytes, current.physicalBytes)}`,
                  ),
                ],
              )
            : undefined,
        ],
      );
    },
  };
};
