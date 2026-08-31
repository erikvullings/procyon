import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { DiskUsageNode, Location, ScanDiskUsageResult } from '../../models';
import { squarify, type TreemapBounds, visibleTreemapChildren } from './treemap-layout';
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
}

const VIEW_BOUNDS: TreemapBounds = { x: 0, y: 0, width: 1000, height: 600 };
const MAX_RENDER_DEPTH = 3;
const DIRECTORY_HEADER_HEIGHT = 22;
const MEDIA = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'mp3', 'wav', 'mp4', 'mov']);
const CODE = new Set(['ts', 'tsx', 'js', 'jsx', 'rs', 'go', 'py', 'java', 'css', 'html']);
const ARCHIVES = new Set(['zip', '7z', 'rar', 'tar', 'gz', 'bz2', 'xz']);
const EXECUTABLES = new Set(['exe', 'dll', 'dylib', 'so', 'app', 'bin']);

export function diskUsageColour(node: DiskUsageNode): string {
  if (node.kind === 'directory') return 'var(--fm-disk-usage-directory)';
  const extension = node.name.includes('.')
    ? (node.name.split('.').at(-1)?.toLowerCase() ?? '')
    : '';
  if (MEDIA.has(extension)) return 'var(--fm-disk-usage-media)';
  if (CODE.has(extension)) return 'var(--fm-disk-usage-code)';
  if (ARCHIVES.has(extension)) return 'var(--fm-disk-usage-archive)';
  if (EXECUTABLES.has(extension)) return 'var(--fm-disk-usage-executable)';
  return 'var(--fm-disk-usage-other)';
}

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

function inset(bounds: TreemapBounds): TreemapBounds {
  const padding = Math.min(2, bounds.width / 8, bounds.height / 8);
  return {
    x: bounds.x + padding,
    y: bounds.y + padding,
    width: Math.max(0, bounds.width - padding * 2),
    height: Math.max(0, bounds.height - padding * 2),
  };
}

function displayLocation(location: Location): string {
  try {
    const url = new URL(location.uri);
    return decodeURIComponent(url.pathname) || location.uri;
  } catch {
    return location.uri;
  }
}

function truncateLabel(name: string, width: number): string {
  const maximumCharacters = Math.max(4, Math.floor((width - 12) / 7));
  return name.length <= maximumCharacters ? name : `${name.slice(0, maximumCharacters - 1)}…`;
}

export const DiskUsageView: FactoryComponent<DiskUsageViewAttrs> = () => {
  let hovered: DiskUsageNode | undefined;
  let hoverPoint:
    | {
        readonly x: number;
        readonly y: number;
        readonly width: number;
        readonly height: number;
      }
    | undefined;
  let warningsOpen = false;
  let elapsedSeconds = 0;
  let progressTimer: ReturnType<typeof setInterval> | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let viewBounds = VIEW_BOUNDS;

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

  function renderNode(
    node: DiskUsageNode,
    bounds: TreemapBounds,
    parentLocation: Location,
    attrs: DiskUsageViewAttrs,
    depth: number,
  ): m.Children {
    const target = node.kind === 'directory' ? node.location : parentLocation;
    const children =
      node.kind === 'directory' && depth < MAX_RENDER_DEPTH
        ? visibleTreemapChildren(node.children, node.physicalBytes)
        : [];
    const showDirectoryHeader =
      node.kind === 'directory' &&
      children.length > 0 &&
      bounds.width > 70 &&
      bounds.height > DIRECTORY_HEADER_HEIGHT * 2;
    const innerBounds = inset(bounds);
    const childBounds = showDirectoryHeader
      ? {
          ...innerBounds,
          y: innerBounds.y + DIRECTORY_HEADER_HEIGHT,
          height: Math.max(0, innerBounds.height - DIRECTORY_HEADER_HEIGHT),
        }
      : innerBounds;
    const showDirectoryLabel =
      node.kind === 'directory' && bounds.width > 70 && bounds.height > DIRECTORY_HEADER_HEIGHT * 2;
    const showLabel =
      showDirectoryLabel ||
      (depth === 0 && children.length === 0 && bounds.width > 75 && bounds.height > 24);
    const activate = () => {
      if (node.collapsed && attrs.state.type === 'loaded' && attrs.state.scanning !== true) {
        attrs.onExpandFolder(node.location);
        return;
      }
      attrs.onOpenFolder(target);
    };
    return m('g', [
      m('rect.fm-disk-usage-block', {
        x: bounds.x,
        y: bounds.y,
        width: Math.max(0, bounds.width),
        height: Math.max(0, bounds.height),
        fill: diskUsageColour(node),
        tabindex: 0,
        role: 'button',
        'aria-label': `${node.name}, ${formatBytes(node.physicalBytes)}`,
        onclick: activate,
        onkeydown: (event: KeyboardEvent) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            activate();
          }
        },
        onpointerenter: (event: PointerEvent) => {
          hovered = node;
          const view = (event.currentTarget as SVGRectElement | null)?.closest(
            '.fm-disk-usage-view',
          );
          const bounds = view?.getBoundingClientRect();
          hoverPoint =
            bounds === undefined
              ? undefined
              : {
                  x: event.clientX - bounds.left,
                  y: event.clientY - bounds.top,
                  width: bounds.width,
                  height: bounds.height,
                };
        },
        onfocus: () => {
          hovered = node;
          hoverPoint = undefined;
        },
        onpointerleave: () => {
          hovered = undefined;
          hoverPoint = undefined;
        },
      }),
      ...squarify(children, childBounds).map((child) =>
        renderNode(child.node, child.bounds, node.location, attrs, depth + 1),
      ),
      showLabel
        ? [
            showDirectoryLabel
              ? m('rect.fm-disk-usage-label-backdrop', {
                  x: innerBounds.x,
                  y: innerBounds.y,
                  width: innerBounds.width,
                  height: DIRECTORY_HEADER_HEIGHT,
                })
              : undefined,
            m(
              'text.fm-disk-usage-label',
              {
                x: innerBounds.x + 6,
                y: innerBounds.y + 15,
              },
              truncateLabel(node.name, innerBounds.width),
            ),
          ]
        : undefined,
    ]);
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
      const children = visibleTreemapChildren(result.root.children, result.root.physicalBytes);
      const unreadable = result.unreadable ?? [];
      const scannedEntries = result.scannedEntries ?? 0;
      return m('.fm-disk-usage-view', [
        m('.fm-disk-usage-toolbar', [
          m('.fm-disk-usage-summary', [
            m('strong', result.root.name),
            m('span.fm-disk-usage-summary-size', formatBytes(result.root.physicalBytes)),
          ]),
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
                  'button.btn.fm-disk-usage-stop',
                  { type: 'button', onclick: attrs.onStop },
                  t('diskUsage', 'stop'),
                ),
              ])
            : undefined,
          result.unreadableEntries > 0
            ? m(
                'button.btn.fm-disk-usage-warning',
                {
                  type: 'button',
                  'aria-expanded': warningsOpen,
                  onclick: () => {
                    warningsOpen = !warningsOpen;
                  },
                },
                t('diskUsage', 'unreadable', { count: result.unreadableEntries }),
              )
            : undefined,
        ]),
        attrs.state.error !== undefined || (warningsOpen && unreadable.length > 0)
          ? m('.fm-disk-usage-notices', [
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
        children.length === 0
          ? m('.fm-disk-usage-status', t('diskUsage', 'empty'))
          : m(
              'svg.fm-disk-usage-map',
              {
                viewBox: `0 0 ${viewBounds.width} ${viewBounds.height}`,
                preserveAspectRatio: 'none',
                role: 'img',
                'aria-label': t('diskUsage', 'treemapLabel', { name: result.root.name }),
                onpointerleave: () => {
                  hovered = undefined;
                  hoverPoint = undefined;
                },
                oncreate: ({ dom }: m.VnodeDOM) => {
                  const updateBounds = () => {
                    const { width, height } = dom.getBoundingClientRect();
                    if (width <= 0 || height <= 0) return;
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
              squarify(children, viewBounds).map((item) =>
                renderNode(item.node, item.bounds, result.root.location, attrs, 0),
              ),
            ),
        hovered !== undefined && hoverPoint !== undefined
          ? m(
              '.fm-disk-usage-tooltip',
              {
                style: {
                  left:
                    hoverPoint.x <= hoverPoint.width / 2
                      ? `${Math.max(8, hoverPoint.x + 12)}px`
                      : undefined,
                  right:
                    hoverPoint.x > hoverPoint.width / 2
                      ? `${Math.max(8, hoverPoint.width - hoverPoint.x + 12)}px`
                      : undefined,
                  top:
                    hoverPoint.y <= hoverPoint.height / 2
                      ? `${Math.max(8, hoverPoint.y + 12)}px`
                      : undefined,
                  bottom:
                    hoverPoint.y > hoverPoint.height / 2
                      ? `${Math.max(8, hoverPoint.height - hoverPoint.y + 12)}px`
                      : undefined,
                },
              },
              [
                m('strong', hovered.name),
                m('span', displayLocation(hovered.location)),
                m(
                  'span',
                  `${t('diskUsage', 'logical')}: ${formatBytes(hovered.logicalBytes)} · ${t('diskUsage', 'physical')}: ${formatBytes(hovered.physicalBytes)}`,
                ),
              ],
            )
          : undefined,
      ]);
    },
  };
};
