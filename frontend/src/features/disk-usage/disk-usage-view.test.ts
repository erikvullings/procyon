import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DiskUsageNode, Location } from '../../models';
import { DiskUsageView, diskUsageAncestors, diskUsageTrail } from './disk-usage-view';

let root: HTMLElement;
let fillText: ReturnType<typeof vi.fn>;

class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}

function stubCanvas(): void {
  fillText = vi.fn();
  vi.stubGlobal('ImageData', FakeImageData);
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
    putImageData: vi.fn(),
    fillText,
    measureText: (text: string) => ({ width: text.length * 6 }),
    font: '',
    textBaseline: 'alphabetic',
    fillStyle: '',
    globalAlpha: 1,
  } as unknown as CanvasRenderingContext2D);
}

function mountLoaded(
  rootNode: DiskUsageNode,
  overrides: {
    onOpenFolder?: (location: Location) => void;
    onExpandFolder?: (location: Location) => void;
  } = {},
): void {
  m.mount(root, {
    view: () =>
      m(DiskUsageView, {
        state: {
          type: 'loaded',
          result: { root: rootNode, unreadableEntries: 0 },
        },
        onOpenFolder: overrides.onOpenFolder ?? vi.fn(),
        onExpandFolder: overrides.onExpandFolder ?? vi.fn(),
        onRetry: vi.fn(),
        onStop: vi.fn(),
      }),
  });
}

function itemButton(name: string): HTMLButtonElement | null {
  return root.querySelector<HTMLButtonElement>(
    `.fm-disk-usage-item-activate[aria-label^="${name},"]`,
  );
}

function directory(name: string, physicalBytes: number): DiskUsageNode {
  return {
    name,
    kind: 'directory',
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    logicalBytes: physicalBytes,
    physicalBytes,
    collapsed: false,
    children: [],
  };
}

beforeEach(() => {
  stubCanvas();
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('DiskUsageView', () => {
  it('keeps the tab responsive while the asynchronous scan is loading', () => {
    vi.useFakeTimers();
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: { type: 'loading', rootName: 'tmp' },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    expect(root.textContent).toContain('Scanning tmp');
    expect(root.textContent).toContain('0 seconds elapsed');

    vi.advanceTimersByTime(3_000);
    m.redraw.sync();
    expect(root.textContent).toContain('3 seconds elapsed');
  });

  it('lets the user stop a scan from its loading state', () => {
    const onStop = vi.fn();
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: { type: 'loading', rootName: 'tmp' },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop,
        }),
    });

    root.querySelector<HTMLButtonElement>('button')?.click();

    expect(onStop).toHaveBeenCalledOnce();
  });

  it('opens an empty directory row through the supplied opposite-pane callback', () => {
    const onOpenFolder = vi.fn();
    const child = directory('projects', 80);
    mountLoaded({ ...directory('tmp', 80), children: [child] }, { onOpenFolder });

    itemButton('projects')?.click();
    expect(onOpenFolder).toHaveBeenCalledWith(child.location);
  });

  it('reveals a file in its parent from both the list and the canvas', () => {
    const onOpenFolder = vi.fn();
    const parent = directory('tmp', 80);
    const file = {
      ...directory('photo.jpg', 80),
      kind: 'file' as const,
    };
    mountLoaded({ ...parent, children: [file] }, { onOpenFolder });

    itemButton('photo.jpg')?.click();
    root
      .querySelector('.fm-disk-usage-canvas')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 500, clientY: 300 }));

    expect(onOpenFolder).toHaveBeenCalledTimes(2);
    expect(onOpenFolder).toHaveBeenNthCalledWith(1, parent.location, 'photo.jpg');
    expect(onOpenFolder).toHaveBeenNthCalledWith(2, parent.location, 'photo.jpg');
  });

  it('shows the list left of the treemap and lets the user hide it', () => {
    mountLoaded({ ...directory('tmp', 80), children: [directory('projects', 80)] });
    const body = root.querySelector('.fm-disk-usage-body');
    expect(body?.firstElementChild?.classList.contains('fm-disk-usage-items')).toBe(true);
    const toggle = root.querySelector<HTMLButtonElement>('.fm-disk-usage-list-toggle');
    expect(toggle?.getAttribute('aria-label')).toBe('Hide list (L)');

    toggle?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-disk-usage-items')).toBeNull();
    expect(root.querySelector('.fm-disk-usage-canvas')).not.toBeNull();
    expect(toggle?.getAttribute('aria-pressed')).toBe('false');
    expect(toggle?.getAttribute('aria-label')).toBe('Show list (L)');

    toggle?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-items')).not.toBeNull();
  });

  it('opens a real directory even when its name resembles the aggregate label', () => {
    const onOpenFolder = vi.fn();
    const child = directory('Small files (archive)', 80);
    mountLoaded({ ...directory('tmp', 80), children: [child] }, { onOpenFolder });

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-item-open')?.click();
    expect(onOpenFolder).toHaveBeenCalledWith(child.location);
  });

  it('shows complete hover details in the tooltip without a redundant footer row', () => {
    const child = directory('projects', 80);
    mountLoaded({ ...directory('tmp', 80), children: [child] });

    root
      .querySelector('.fm-disk-usage-item')
      ?.dispatchEvent(new MouseEvent('pointerenter', { bubbles: true }));
    m.redraw.sync();
    const tooltip = root.querySelector('.fm-disk-usage-tooltip')?.textContent;
    expect(tooltip).toContain('/tmp/projects');
    expect(tooltip).toContain('Logical');
    expect(tooltip).toContain('Physical');
    expect(root.querySelector('.fm-disk-usage-details')).toBeNull();
    expect(root.querySelector('title')).toBeNull();
    expect(root.querySelector('.fm-disk-usage-highlight')).not.toBeNull();
  });

  it('hit-tests the canvas to show the tile under the pointer', () => {
    const photo = { ...directory('photo.jpg', 80), kind: 'file' as const };
    const pictures = {
      ...directory('pictures', 80),
      location: { providerId: 'local', uri: 'file:///tmp/pictures/' },
      children: [
        {
          ...photo,
          location: { providerId: 'local', uri: 'file:///tmp/pictures/photo.jpg' },
        },
      ],
    };
    mountLoaded({ ...directory('tmp', 80), children: [pictures] });

    root
      .querySelector('.fm-disk-usage-canvas')
      ?.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 500, clientY: 300 }));
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-tooltip')?.textContent).toContain(
      '/tmp/pictures/photo.jpg',
    );
  });

  it('zooms into a folder from the canvas, the list and back out via breadcrumbs and keys', () => {
    const onOpenFolder = vi.fn();
    const file = (name: string, bytes: number, parent: string) => ({
      ...directory(name, bytes),
      kind: 'file' as const,
      location: { providerId: 'local', uri: `file:///tmp/${parent}/${name}` },
    });
    const inner = {
      ...directory('inner', 60),
      location: { providerId: 'local', uri: 'file:///tmp/projects/inner/' },
      children: [file('a.bin', 30, 'projects/inner'), file('b.bin', 30, 'projects/inner')],
    };
    const projects = {
      ...directory('projects', 100),
      location: { providerId: 'local', uri: 'file:///tmp/projects/' },
      children: [inner, file('notes.txt', 40, 'projects')],
    };
    mountLoaded({ ...directory('tmp', 100), children: [projects] }, { onOpenFolder });

    root
      .querySelector('.fm-disk-usage-canvas')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 500, clientY: 300 }));
    m.redraw.sync();
    expect(root.querySelector('[aria-current="location"]')?.textContent).toBe('projects');
    expect(onOpenFolder).toHaveBeenCalledWith(projects.location);

    itemButton('inner')?.click();
    m.redraw.sync();
    expect(root.querySelector('[aria-current="location"]')?.textContent).toBe('inner');
    expect(onOpenFolder).toHaveBeenCalledWith(inner.location);
    expect(itemButton('a.bin')).not.toBeNull();

    root
      .querySelector('.fm-disk-usage-view')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('[aria-current="location"]')?.textContent).toBe('projects');

    root.querySelector<HTMLButtonElement>('button.fm-disk-usage-crumb')?.click();
    m.redraw.sync();
    expect(root.querySelector('[aria-current="location"]')?.textContent).toBe('tmp');
    expect(root.querySelector('button.fm-disk-usage-crumb')).toBeNull();
  });

  it('opens the zoomed folder in the other pane', () => {
    const onOpenFolder = vi.fn();
    mountLoaded(
      { ...directory('tmp', 80), children: [directory('projects', 80)] },
      {
        onOpenFolder,
      },
    );

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-open-current')?.click();
    expect(onOpenFolder).toHaveBeenCalledWith({ providerId: 'local', uri: 'file:///tmp/tmp' });
  });

  it('finds the zoom trail by location and falls back to the root', () => {
    const inner = {
      ...directory('inner', 10),
      location: { providerId: 'local', uri: 'file:///tmp/a/inner/' },
    };
    const a = {
      ...directory('a', 10),
      location: { providerId: 'local', uri: 'file:///tmp/a/' },
      children: [inner],
    };
    const tree = { ...directory('tmp', 10), children: [a] };
    expect(diskUsageTrail(tree, inner.location.uri).map((node) => node.name)).toEqual([
      'tmp',
      'a',
      'inner',
    ]);
    expect(diskUsageTrail(tree, 'file:///elsewhere').map((node) => node.name)).toEqual(['tmp']);
  });

  it('shows the full path as pane breadcrumbs and the size in the status bar', () => {
    const onScanFolder = vi.fn();
    const projects = { ...directory('projects', 60), children: [directory('inner', 60)] };
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            result: {
              root: {
                ...directory('tmp', 80),
                location: { providerId: 'local', uri: 'file:///tmp' },
                children: [projects, directory('other', 20)],
              },
              unreadableEntries: 0,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
          onScanFolder,
        }),
    });

    const crumbs = () =>
      [...root.querySelectorAll('.fm-breadcrumb-row .fm-breadcrumb-segment')].map(
        (crumb) => crumb.textContent,
      );
    expect(crumbs()).toEqual(['/', 'tmp']);
    expect(root.querySelector('.fm-pane-status .fm-disk-usage-total')?.textContent).toBe('80 B');

    itemButton('projects')?.click();
    m.redraw.sync();
    expect(crumbs()).toEqual(['/', 'tmp', 'projects']);
    expect(root.querySelector('.fm-disk-usage-total')?.textContent).toBe('60 B of 80 B (75%)');

    root.querySelector<HTMLButtonElement>('button.fm-disk-usage-ancestor')?.click();
    expect(onScanFolder).toHaveBeenCalledWith({ providerId: 'local', uri: 'file:///' });
  });

  it('toggles the list with the L key and describes the shortcut', () => {
    mountLoaded({ ...directory('tmp', 80), children: [directory('projects', 80)] });
    expect(
      root
        .querySelector('.fm-disk-usage-list-toggle')
        ?.closest('[data-tooltip]')
        ?.getAttribute('data-tooltip'),
    ).toBe('Hide list (L)');

    const view = root.querySelector('.fm-disk-usage-view');
    view?.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-items')).toBeNull();

    view?.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-items')).toBeNull();

    view?.dispatchEvent(new KeyboardEvent('keydown', { key: 'L', bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-items')).not.toBeNull();
  });

  it('lists the ancestors of a scanned folder for POSIX and drive-letter URIs', () => {
    expect(
      diskUsageAncestors({ providerId: 'local', uri: 'file:///Users/me/My%20Files' }).map(
        (crumb) => [crumb.label, crumb.location.uri],
      ),
    ).toEqual([
      ['/', 'file:///'],
      ['Users', 'file:///Users'],
      ['me', 'file:///Users/me'],
    ]);
    expect(
      diskUsageAncestors({ providerId: 'local', uri: 'file:///C:/Data/big' }).map(
        (crumb) => crumb.label,
      ),
    ).toEqual(['C:', 'Data']);
    expect(diskUsageAncestors({ providerId: 'local', uri: 'file:///' })).toEqual([]);
  });

  it('shows scan activity and lets the user stop from a progressive result', () => {
    vi.useFakeTimers();
    const onStop = vi.fn();
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            scanning: true,
            result: {
              root: { ...directory('tmp', 80), children: [directory('projects', 80)] },
              unreadableEntries: 0,
              unreadable: [],
              scannedEntries: 12_345,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop,
        }),
    });

    expect(root.querySelector('.fm-disk-usage-progress')?.textContent).toContain(
      new Intl.NumberFormat().format(12_345),
    );
    root.querySelector<HTMLButtonElement>('.fm-disk-usage-stop')?.click();
    expect(onStop).toHaveBeenCalledOnce();
  });

  it('explains when traversal is complete and the final tree is being assembled', () => {
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            scanning: true,
            finalizing: true,
            result: {
              root: { ...directory('tmp', 80), children: [directory('projects', 80)] },
              unreadableEntries: 0,
              scannedEntries: 4_302_322,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    expect(root.querySelector('.fm-disk-usage-progress')?.textContent).toContain('Finalizing');
    expect(root.querySelector('.fm-disk-usage-progress')?.textContent).toContain(
      new Intl.NumberFormat().format(4_302_322),
    );
  });

  it('lists unreadable paths and their sanitized reasons', () => {
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            result: {
              root: directory('tmp', 80),
              unreadableEntries: 1,
              unreadable: [
                {
                  location: { providerId: 'local', uri: 'file:///tmp/private' },
                  reason: 'permissionDenied',
                },
              ],
              scannedEntries: 10,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-warning')?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-disk-usage-warnings')?.textContent).toContain('/tmp/private');
    expect(root.querySelector('.fm-disk-usage-warnings')?.textContent).toContain(
      'Permission denied',
    );

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-warning')?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-disk-usage-warnings')).toBeNull();

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-warning')?.click();
    m.redraw.sync();
    root
      .querySelector<HTMLButtonElement>('.fm-disk-usage-warnings .fm-disk-usage-notice-close')
      ?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-disk-usage-warnings')).toBeNull();
    expect(root.querySelector('.fm-disk-usage-warning')?.getAttribute('aria-expanded')).toBe(
      'false',
    );

    root.querySelector<HTMLButtonElement>('.fm-disk-usage-warning')?.click();
    m.redraw.sync();
    root
      .querySelector('.fm-disk-usage-view')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();

    expect(root.querySelector('.fm-disk-usage-warnings')).toBeNull();
  });

  it('prioritizes folder labels over deeply nested hash filenames', () => {
    const hash = {
      ...directory('sha256-deadbeef', 80),
      kind: 'file' as const,
    };
    const cache = {
      ...directory('model-cache', 80),
      children: [hash],
    };
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            result: {
              root: { ...directory('tmp', 80), children: [cache] },
              unreadableEntries: 0,
              unreadable: [],
              scannedEntries: 2,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    const labels = fillText.mock.calls.map(([text]) => text);
    expect(labels).toContain('model-cache');
    expect(labels).not.toContain('sha256-deadbeef');
  });

  it('expands a collapsed directory when its row is activated', () => {
    const onExpandFolder = vi.fn();
    const child = { ...directory('node_modules', 80), collapsed: true };
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            result: {
              root: { ...directory('tmp', 80), children: [child] },
              unreadableEntries: 0,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder,
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    itemButton('node_modules')?.click();

    expect(onExpandFolder).toHaveBeenCalledWith(child.location);
    expect(root.querySelector('.fm-disk-usage-details')).toBeNull();
  });

  describe('clean-up candidates', () => {
    const candidate = {
      location: { providerId: 'local', uri: 'file:///tmp/app/node_modules' },
      kind: 'nodeModules' as const,
      logicalBytes: 60,
      physicalBytes: 60,
    };

    function mountWithCandidates(
      onOpenFolder: (location: Location) => void,
      onTrashFolder?: (location: Location) => Promise<boolean>,
    ): void {
      m.mount(root, {
        view: () =>
          m(DiskUsageView, {
            state: {
              type: 'loaded',
              result: {
                root: { ...directory('tmp', 80), children: [directory('app', 80)] },
                unreadableEntries: 0,
                cleanupCandidates: [candidate],
              },
            },
            onOpenFolder,
            onExpandFolder: vi.fn(),
            onRetry: vi.fn(),
            onStop: vi.fn(),
            ...(onTrashFolder === undefined ? {} : { onTrashFolder }),
          }),
      });
    }

    it('lists candidates with their rule and lets the user show one in the other pane', () => {
      const onOpenFolder = vi.fn();
      mountWithCandidates(onOpenFolder);

      const toggle = root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-toggle');
      expect(toggle?.textContent).toContain('1');
      expect(root.querySelector('.fm-disk-usage-cleanup')).toBeNull();
      toggle?.click();
      m.redraw.sync();

      const panel = root.querySelector('.fm-disk-usage-cleanup');
      expect(panel?.textContent).toContain('/tmp/app/node_modules');
      expect(panel?.textContent).toContain('JavaScript dependencies');
      expect(root.querySelector('.fm-disk-usage-cleanup-trash')).toBeNull();
      root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-show')?.click();
      expect(onOpenFolder).toHaveBeenCalledWith(candidate.location);
    });

    it('hides a candidate once its move to Trash has started', async () => {
      const onTrashFolder = vi.fn(() => Promise.resolve(true));
      mountWithCandidates(vi.fn(), onTrashFolder);
      root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-toggle')?.click();
      m.redraw.sync();

      root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-trash')?.click();
      expect(onTrashFolder).toHaveBeenCalledWith(candidate.location);
      await vi.waitFor(() => {
        m.redraw.sync();
        expect(root.querySelector('.fm-disk-usage-cleanup-toggle')).toBeNull();
      });
    });

    it('keeps a candidate when the user declines the Trash confirmation', async () => {
      const onTrashFolder = vi.fn(() => Promise.resolve(false));
      mountWithCandidates(vi.fn(), onTrashFolder);
      root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-toggle')?.click();
      m.redraw.sync();

      root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-trash')?.click();
      await vi.waitFor(() => {
        m.redraw.sync();
        expect(
          root.querySelector<HTMLButtonElement>('.fm-disk-usage-cleanup-trash')?.disabled,
        ).toBe(false);
      });
      expect(root.querySelector('.fm-disk-usage-cleanup')).not.toBeNull();
    });
  });
});
