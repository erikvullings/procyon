import m, { type VnodeDOM } from 'mithril';
import { t } from '../../i18n';
import type { SearchPresentation } from '../search/search-presentation';

interface BreadcrumbTrailAttrs {
  readonly pathKey: string;
}

/** Keeps the current segment in view without removing navigable ancestors from the DOM. */
export const BreadcrumbTrail: m.ClosureComponent<BreadcrumbTrailAttrs> = () => {
  let trail: HTMLElement | undefined;
  let observer: ResizeObserver | undefined;
  let displayedPath: string | undefined;
  let overflowed = false;
  let clipped = false;

  function updateClipping(): void {
    if (trail === undefined) return;
    const nextOverflowed = trail.scrollWidth > trail.clientWidth + 1;
    const nextClipped = nextOverflowed && trail.scrollLeft > 1;
    if (nextClipped !== clipped || nextOverflowed !== overflowed) {
      clipped = nextClipped;
      overflowed = nextOverflowed;
      m.redraw();
    }
  }

  function showEnd(): void {
    if (trail === undefined) return;
    trail.scrollLeft = Math.max(0, trail.scrollWidth - trail.clientWidth);
    updateClipping();
  }

  return {
    view: ({ attrs, children }) =>
      m('.fm-breadcrumb-trail', [
        m(
          'button.fm-breadcrumb-overflow',
          {
            type: 'button',
            hidden: !clipped,
            'aria-label': t('pane', 'showPathBeginning'),
            onclick: () => {
              if (trail === undefined) return;
              trail.scrollLeft = 0;
              updateClipping();
              trail.focus();
            },
          },
          '…',
        ),
        m(
          '.fm-breadcrumb-segments',
          {
            tabindex: overflowed ? 0 : undefined,
            oncreate: ({ dom }: VnodeDOM) => {
              trail = dom as HTMLElement;
              displayedPath = attrs.pathKey;
              if (typeof ResizeObserver !== 'undefined') {
                observer = new ResizeObserver(showEnd);
                observer.observe(trail);
              }
              showEnd();
            },
            onupdate: () => {
              if (displayedPath !== attrs.pathKey) {
                displayedPath = attrs.pathKey;
                showEnd();
              }
            },
            onscroll: updateClipping,
            onremove: () => {
              observer?.disconnect();
              observer = undefined;
              trail = undefined;
            },
          },
          children,
        ),
      ]),
  };
};

/** A cumulative, clickable part of a filesystem path. */
export interface BreadcrumbSegment {
  readonly label: string;
  readonly path: string;
}

function posixSegments(path: string): readonly BreadcrumbSegment[] {
  if (path === '/') {
    return [{ label: '/', path: '/' }];
  }
  const parts = path.split('/').filter(Boolean);
  if (parts[0] === '~') {
    return parts.map((label, index) => ({
      label,
      path: index === 0 ? '~' : `~/${parts.slice(1, index + 1).join('/')}`,
    }));
  }
  // Windows `file:` paths arrive as `/C:/dev`; the drive is the root, as in Explorer.
  if (parts[0] !== undefined && /^[A-Za-z]:$/.test(parts[0])) {
    const drive = parts[0];
    return parts.map((label, index) => ({
      label,
      path: index === 0 ? `/${drive}/` : `/${parts.slice(0, index + 1).join('/')}`,
    }));
  }
  return [
    { label: '/', path: '/' },
    ...parts.map((label, index) => ({
      label,
      path: `/${parts.slice(0, index + 1).join('/')}`,
    })),
  ];
}

function windowsSegments(path: string): readonly BreadcrumbSegment[] {
  const separator = '\\';
  const parts = path.split(separator).filter(Boolean);
  if (path.startsWith('\\\\') && parts.length >= 2) {
    const root = `\\\\${parts[0]}\\${parts[1]}`;
    return [
      { label: root, path: root },
      ...parts.slice(2).map((label, index) => ({
        label,
        path: `${root}\\${parts.slice(2, index + 3).join('\\')}`,
      })),
    ];
  }
  const root = parts[0] ?? path;
  return [
    { label: root, path: root.endsWith(':') ? `${root}\\` : root },
    ...parts.slice(1).map((label, index) => ({
      label,
      path: `${root}\\${parts.slice(1, index + 2).join('\\')}`,
    })),
  ];
}

/** Produces cumulative breadcrumb targets for POSIX, drive-letter and UNC paths. */
export function breadcrumbSegments(path: string): readonly BreadcrumbSegment[] {
  return path.includes('\\') ? windowsSegments(path) : posixSegments(path);
}

/** A search is a virtual result set, not a navigable filesystem path. */
export function searchResultTitle(presentation: SearchPresentation | undefined): string {
  const term = presentation?.headerTerm ?? presentation?.label ?? presentation?.term;
  return term ? `${t('search', 'resultsTitle')} · ${term}` : t('search', 'resultsTitle');
}
