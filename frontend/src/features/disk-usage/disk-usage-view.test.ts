import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DiskUsageNode } from '../../models';
import { DiskUsageView } from './disk-usage-view';

let root: HTMLElement;

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
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.useRealTimers();
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

  it('opens a clicked directory block through the supplied opposite-pane callback', () => {
    const onOpenFolder = vi.fn();
    const child = directory('projects', 80);
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
          onOpenFolder,
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    root
      .querySelector<SVGRectElement>('.fm-disk-usage-block')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onOpenFolder).toHaveBeenCalledWith(child.location);
  });

  it('opens a real directory even when its name resembles the aggregate label', () => {
    const onOpenFolder = vi.fn();
    const child = directory('Small files (archive)', 80);
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
          onOpenFolder,
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    root
      .querySelector<SVGRectElement>('.fm-disk-usage-block')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onOpenFolder).toHaveBeenCalledWith(child.location);
  });

  it('shows complete hover details in the tooltip without a redundant footer row', () => {
    const child = directory('projects', 80);
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
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    root
      .querySelector<SVGRectElement>('.fm-disk-usage-block')
      ?.dispatchEvent(new MouseEvent('pointerenter', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-disk-usage-tooltip')?.textContent).toContain('/tmp/projects');
    expect(root.querySelector('.fm-disk-usage-tooltip')?.textContent).toContain('Logical');
    expect(root.querySelector('.fm-disk-usage-tooltip')?.textContent).toContain('Physical');
    expect(root.querySelector('.fm-disk-usage-details')).toBeNull();
    expect(root.querySelector('title')).toBeNull();
  });

  it('stacks the root size beneath its folder name', () => {
    m.mount(root, {
      view: () =>
        m(DiskUsageView, {
          state: {
            type: 'loaded',
            result: {
              root: { ...directory('tmp', 80), children: [directory('projects', 80)] },
              unreadableEntries: 0,
            },
          },
          onOpenFolder: vi.fn(),
          onExpandFolder: vi.fn(),
          onRetry: vi.fn(),
          onStop: vi.fn(),
        }),
    });

    expect(root.querySelector('.fm-disk-usage-summary > strong')?.textContent).toBe('tmp');
    expect(root.querySelector('.fm-disk-usage-summary-size')?.textContent).toBe('80 B');
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

    const labels = [...root.querySelectorAll('.fm-disk-usage-label')].map(
      (label) => label.textContent,
    );
    expect(labels).toContain('model-cache');
    expect(labels).not.toContain('sha256-deadbeef');
  });

  it('expands a collapsed directory when its block is activated', () => {
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

    root
      .querySelector<SVGRectElement>('.fm-disk-usage-block')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onExpandFolder).toHaveBeenCalledWith(child.location);
    expect(root.querySelector('.fm-disk-usage-details')).toBeNull();
  });
});
