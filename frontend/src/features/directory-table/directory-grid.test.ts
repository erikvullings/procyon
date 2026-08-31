import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntrySummary } from '../../models';
import { DirectoryGrid, type DirectoryGridAttrs } from './directory-grid';
import { entryArraySource } from './directory-table';
import type { NativeIconLoader } from './native-icon-loader';
import type { ThumbnailLoader } from './thumbnail-loader';

let root: HTMLElement;

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'entry-1',
    location: { providerId: 'file', uri: 'mock:///photo.png' },
    name: 'photo.png',
    kind: 'file',
    size: 1_024,
    modifiedAt: '2026-07-30T12:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension: 'png',
    metadataRevision: 1,
    ...overrides,
  };
}

function mount(attrs: DirectoryGridAttrs): void {
  m.mount(root, { view: () => m(DirectoryGrid, attrs) });
}

function thumbnailLoaderReturning(dataUri: string | undefined): ThumbnailLoader {
  return {
    createViewport: () => ({
      beginFrame: vi.fn(),
      thumbnailDataUri: vi.fn().mockReturnValue(dataUri),
      endFrame: vi.fn(),
      dispose: vi.fn(),
    }),
  } as unknown as ThumbnailLoader;
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('DirectoryGrid', () => {
  it('overlays a loaded thumbnail and otherwise keeps the themed icon', () => {
    const thumbnailLoader = thumbnailLoaderReturning('data:image/jpeg;base64,/9j/4A==');
    mount({
      state: { type: 'loaded' },
      source: entryArraySource([entry()]),
      thumbnailLoader,
    });

    const image = root.querySelector<HTMLImageElement>('img.fm-grid-thumbnail');
    expect(image?.src).toBe('data:image/jpeg;base64,/9j/4A==');
    expect(root.querySelector('.fm-grid-icon')).toBeNull();
  });

  it('falls back to the native icon while no thumbnail is available', () => {
    const nativeIconLoader = {
      iconDataUri: vi.fn().mockReturnValue('data:image/png;base64,iVBORw=='),
    } as unknown as NativeIconLoader;
    const thumbnailLoader = thumbnailLoaderReturning(undefined);
    mount({
      state: { type: 'loaded' },
      source: entryArraySource([entry()]),
      nativeIconLoader,
      thumbnailLoader,
    });

    expect(root.querySelector('img.fm-native-grid-icon')).not.toBeNull();
    expect(root.querySelector('img.fm-grid-thumbnail')).toBeNull();
  });

  it('falls back to the themed glyph icon when neither loader has anything', () => {
    mount({ state: { type: 'loaded' }, source: entryArraySource([entry()]) });

    expect(root.querySelector('.fm-grid-icon')).not.toBeNull();
    expect(root.querySelector('img.fm-grid-thumbnail')).toBeNull();
  });

  it('requests the requested icon size from the thumbnail loader', () => {
    const thumbnailDataUri = vi.fn().mockReturnValue(undefined);
    const beginFrame = vi.fn();
    const endFrame = vi.fn();
    const dispose = vi.fn();
    const thumbnailLoader = {
      createViewport: vi.fn().mockReturnValue({
        beginFrame,
        thumbnailDataUri,
        endFrame,
        dispose,
      }),
    } as unknown as ThumbnailLoader;
    mount({
      state: { type: 'loaded' },
      source: entryArraySource([entry()]),
      iconSize: 'large',
      thumbnailLoader,
    });

    expect(beginFrame).toHaveBeenCalledTimes(1);
    expect(thumbnailDataUri).toHaveBeenCalledWith(expect.anything(), 'large');
    expect(endFrame).toHaveBeenCalledTimes(1);

    m.mount(root, null);
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it('shows the filename below the tile', () => {
    mount({
      state: { type: 'loaded' },
      source: entryArraySource([entry({ name: 'holiday.jpg' })]),
    });

    expect(root.querySelector('.fm-grid-tile-name')?.textContent).toBe('holiday.jpg');
  });

  it('marks the cursor and selected tiles', () => {
    const entries = [entry({ id: 'a' }), entry({ id: 'b', name: 'b.png' })];
    mount({
      state: { type: 'loaded' },
      source: entryArraySource(entries),
      cursorIndex: 0,
      selectedEntryIds: new Set(['b']),
    });

    const tiles = root.querySelectorAll('.fm-grid-tile');
    expect(tiles[0]?.classList.contains('fm-cursor-tile')).toBe(true);
    expect(tiles[1]?.classList.contains('fm-selected-tile')).toBe(true);
    expect(tiles[1]?.getAttribute('aria-selected')).toBe('true');
  });

  it('calls onCursorChange on click and onActivate on double-click', () => {
    const onCursorChange = vi.fn();
    const onActivate = vi.fn();
    mount({
      state: { type: 'loaded' },
      source: entryArraySource([entry()]),
      onCursorChange,
      onActivate,
    });

    const tile = root.querySelector<HTMLElement>('.fm-grid-tile');
    tile?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onCursorChange).toHaveBeenCalledWith(0, { shiftKey: false, ctrlKey: false });

    tile?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(onActivate).toHaveBeenCalledWith(0);
  });

  it('calls onContextMenu with coordinates and prevents the default menu', () => {
    const onContextMenu = vi.fn();
    mount({ state: { type: 'loaded' }, source: entryArraySource([entry()]), onContextMenu });

    const tile = root.querySelector<HTMLElement>('.fm-grid-tile');
    const event = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: 10,
      clientY: 20,
    });
    tile?.dispatchEvent(event);

    expect(onContextMenu).toHaveBeenCalledWith(0, 10, 20);
    expect(event.defaultPrevented).toBe(true);
  });

  it('renders an accessible loading status while empty', () => {
    mount({ state: { type: 'loading' } });
    expect(root.querySelector('[role="status"]')?.textContent).toContain('Loading directory');
  });

  it('renders an accessible error state with a retry button', () => {
    const onRetry = vi.fn();
    mount({ state: { type: 'error', message: 'network down' }, onRetry });

    const alert = root.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain('network down');
    root.querySelector<HTMLButtonElement>('.fm-directory-retry')?.click();
    expect(onRetry).toHaveBeenCalled();
  });

  it('renders an empty-directory message once loaded with no entries', () => {
    mount({ state: { type: 'loaded' }, source: entryArraySource([]) });
    expect(root.querySelector('[role="status"]')?.textContent).toBe('This directory is empty.');
  });

  describe('photo mode', () => {
    it('inserts a day header before tiles from a new day and none between same-day tiles', () => {
      const entries = [
        entry({ id: 'a', modifiedAt: '2026-07-30T09:00:00.000Z' }),
        entry({ id: 'b', name: 'b.png', modifiedAt: '2026-07-30T18:00:00.000Z' }),
        entry({ id: 'c', name: 'c.png', modifiedAt: '2026-07-29T09:00:00.000Z' }),
      ];
      mount({ state: { type: 'loaded' }, source: entryArraySource(entries), photoMode: true });

      const headers = root.querySelectorAll('.fm-grid-day-header');
      const tiles = root.querySelectorAll('.fm-grid-tile');
      expect(headers).toHaveLength(2);
      expect(tiles).toHaveLength(3);
    });

    it('renders no day headers when photo mode is off', () => {
      const entries = [
        entry({ id: 'a', modifiedAt: '2026-07-30T09:00:00.000Z' }),
        entry({ id: 'b', name: 'b.png', modifiedAt: '2026-07-29T09:00:00.000Z' }),
      ];
      mount({ state: { type: 'loaded' }, source: entryArraySource(entries) });

      expect(root.querySelectorAll('.fm-grid-day-header')).toHaveLength(0);
      expect(root.querySelectorAll('.fm-grid-tile')).toHaveLength(2);
    });

    it('still renders tile content (thumbnail/icon/name) for grouped tiles', () => {
      mount({
        state: { type: 'loaded' },
        source: entryArraySource([entry({ name: 'holiday.jpg' })]),
        photoMode: true,
      });

      expect(root.querySelector('.fm-grid-tile-name')?.textContent).toBe('holiday.jpg');
    });

    it('groups an entry with a missing modifiedAt under a single "Unknown date" header', () => {
      const withoutDate = (overrides: Partial<EntrySummary>): EntrySummary => {
        const { modifiedAt: _unused, ...rest } = entry(overrides);
        return rest as EntrySummary;
      };
      const entries = [withoutDate({ id: 'a' }), withoutDate({ id: 'b', name: 'b.png' })];
      mount({ state: { type: 'loaded' }, source: entryArraySource(entries), photoMode: true });

      const headers = root.querySelectorAll('.fm-grid-day-header');
      expect(headers).toHaveLength(1);
      expect(headers[0]?.textContent).toBe('Unknown date');
    });
  });
});
