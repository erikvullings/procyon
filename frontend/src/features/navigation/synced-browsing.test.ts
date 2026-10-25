import { describe, expect, it, vi } from 'vitest';
import type { Location, PaneId, WorkspaceProjection } from '../../models';
import { type NavigationController, parentLocation } from './navigation';
import {
  ancestorLocation,
  createLocalStorageSyncStore,
  createSyncedBrowsing,
  joinLocation,
  locationPath,
  relativeSegments,
  type SyncBrowsingStore,
  sameLocationPath,
} from './synced-browsing';

const local = (path: string): Location => ({ providerId: 'local', uri: `file://${path}` });
const remote = (path: string): Location => ({ providerId: 'sftp', uri: `sftp://host${path}` });
const archive = (outer: string, inner: string): Location => ({
  providerId: 'archive',
  uri: `archive://${outer}!/${inner}`,
});

function defined<T>(value: T | undefined): T {
  if (value === undefined) throw new Error('expected a defined value');
  return value;
}

describe('location path helpers', () => {
  it('splits and joins file, remote and archive locations on segments', () => {
    const file = defined(locationPath(local('/a/My%20Dir/b')));
    expect(file.segments).toEqual(['a', 'My Dir', 'b']);
    expect(joinLocation(file, ['a', 'My Dir']).uri).toBe('file:///a/My%20Dir');
    expect(locationPath(remote('/x/y'))?.segments).toEqual(['x', 'y']);
    const arc = defined(locationPath(archive('/tmp/z.zip', 'in/ner')));
    expect(arc.segments).toEqual(['in', 'ner']);
    expect(joinLocation(arc, ['in']).uri).toBe('archive:///tmp/z.zip!/in');
  });

  it('has no path for search results and the basket', () => {
    expect(locationPath({ providerId: 'search', uri: 'search://local/abc' })).toBeUndefined();
    expect(locationPath({ providerId: 'basket', uri: 'basket://x' })).toBeUndefined();
  });

  it('computes relative segments, case-insensitively, only inside the root', () => {
    expect(relativeSegments(local('/Data'), local('/data/Sub/Deep'))).toEqual(['Sub', 'Deep']);
    expect(relativeSegments(local('/data'), local('/data'))).toEqual([]);
    expect(relativeSegments(local('/data/a'), local('/data'))).toBeUndefined();
    expect(relativeSegments(local('/data'), remote('/data/a'))).toBeUndefined();
  });

  it('compares locations by segments and walks ancestors', () => {
    expect(sameLocationPath(local('/A/b'), local('/a/B'))).toBe(true);
    expect(sameLocationPath(local('/a/b'), local('/a'))).toBe(false);
    expect(ancestorLocation(local('/a/b/c'), 2)?.uri).toBe('file:///a');
    expect(ancestorLocation(local('/a'), 3)).toBeUndefined();
  });
});

interface Harness {
  readonly sync: ReturnType<typeof createSyncedBrowsing>;
  readonly locations: Record<string, Location>;
  readonly store: SyncBrowsingStore;
  readonly navigate: ReturnType<typeof vi.fn>;
  readonly onChange: ReturnType<typeof vi.fn>;
}

/** `dirs` lists the existing directories of the fake file system (all providers). */
function harness(options: {
  left: Location;
  right: Location;
  dirs: readonly string[];
  stored?: boolean;
}): Harness {
  const locations: Record<string, Location> = { left: options.left, right: options.right };
  const history: Record<string, Location[]> = { left: [], right: [] };
  const forward: Record<string, Location[]> = { left: [], right: [] };
  const known = options.dirs.map((d) => d.toLowerCase());
  const workspace = (): WorkspaceProjection =>
    ({
      id: 'ws',
      paneOrder: ['left', 'right'],
      panesById: Object.fromEntries(
        Object.entries(locations).map(([id, location]) => [
          id,
          { activeTabId: 't', tabsById: { t: { location } } },
        ]),
      ),
    }) as unknown as WorkspaceProjection;
  const push = (paneId: PaneId, location: Location): Promise<void> => {
    history[paneId]?.push(locations[paneId] as Location);
    forward[paneId] = [];
    locations[paneId] = location;
    return Promise.resolve();
  };
  const navigate = vi.fn(async (paneId: PaneId, location: Location) => push(paneId, location));
  const navigation = {
    navigate,
    parent: (paneId: PaneId) => push(paneId, parentLocation(locations[paneId] as Location)),
    back: (paneId: PaneId) => {
      const previous = history[paneId]?.pop();
      if (previous !== undefined) {
        forward[paneId]?.push(locations[paneId] as Location);
        locations[paneId] = previous;
      }
      return Promise.resolve();
    },
    forward: (paneId: PaneId) => {
      const next = forward[paneId]?.pop();
      if (next !== undefined) {
        history[paneId]?.push(locations[paneId] as Location);
        locations[paneId] = next;
      }
      return Promise.resolve();
    },
  } as unknown as NavigationController;
  let flag = options.stored ?? false;
  const store: SyncBrowsingStore = {
    load: () => flag,
    save: (_id, enabled) => {
      flag = enabled;
    },
  };
  const onChange = vi.fn();
  const sync = createSyncedBrowsing({
    navigation,
    getWorkspace: workspace,
    store,
    onChange,
    resolveChild: (parent, name) => {
      const parentPath = locationPath(parent);
      if (parentPath === undefined) return Promise.resolve(undefined);
      const wanted = [...parentPath.segments, name];
      const key = `${parentPath.providerId}:${wanted.join('/')}`.toLowerCase();
      if (!known.includes(key)) return Promise.resolve(undefined);
      const actual = options.dirs.find((d) => d.toLowerCase() === key) as string;
      return Promise.resolve(joinLocation(parentPath, (actual.split(':')[1] ?? '').split('/')));
    },
  });
  return { sync, locations, store, navigate, onChange };
}

const DIRS = [
  'local:a',
  'local:a/src',
  'local:a/src/deep',
  'local:b',
  'local:b/src',
  'local:b/src/deep',
  'local:b/Docs',
  'local:a/only-left',
];

describe('synchronized browsing', () => {
  it('does nothing while disabled', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right).toEqual(local('/b'));
    expect(h.sync.state()).toBe('off');
  });

  it('mirrors entering a subfolder, going to the parent, and back/forward', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right?.uri).toBe('file:///b/src');
    await h.sync.navigation.navigate('left', local('/a/src/deep'));
    expect(h.locations.right?.uri).toBe('file:///b/src/deep');
    await h.sync.navigation.parent('left');
    expect(h.locations.right?.uri).toBe('file:///b/src');
    await h.sync.navigation.back('left');
    expect(h.locations.right?.uri).toBe('file:///b/src/deep');
    await h.sync.navigation.forward('left');
    expect(h.locations.right?.uri).toBe('file:///b/src');
    expect(h.sync.state()).toBe('synced');
  });

  it('mirrors in both directions', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('right', local('/b/src'));
    expect(h.locations.left?.uri).toBe('file:///a/src');
  });

  it('desynchronizes without error when the counterpart is missing, then resyncs', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/a/only-left'));
    expect(h.locations.right).toEqual(local('/b'));
    expect(h.sync.state()).toBe('desynced');
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right?.uri).toBe('file:///b/src');
    expect(h.sync.state()).toBe('synced');
  });

  it('resyncs when the lagging pane is moved to the aligned place by hand', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/a/only-left'));
    expect(h.sync.state()).toBe('desynced');
    await h.sync.navigation.parent('left');
    expect(h.sync.state()).toBe('synced');
  });

  it('matches names case-insensitively and keeps the counterpart spelling', async () => {
    const h = harness({
      left: local('/a'),
      right: local('/b'),
      dirs: [...DIRS, 'local:a/docs'],
    });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/a/docs'));
    expect(h.locations.right?.uri).toBe('file:///b/Docs');
  });

  it('moves the other pane up when the pane goes above its starting folder', async () => {
    const h = harness({ left: local('/a/src'), right: local('/b/src'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.parent('left');
    expect(h.locations.right?.uri).toBe('file:///b');
    // Re-anchored one level up: entering again keeps mirroring.
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right?.uri).toBe('file:///b/src');
  });

  it('desynchronizes on an unrelated jump and leaves the other pane alone', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/elsewhere'));
    expect(h.locations.right).toEqual(local('/b'));
    expect(h.sync.state()).toBe('desynced');
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right?.uri).toBe('file:///b/src');
    expect(h.sync.state()).toBe('synced');
  });

  it('works across providers (local to remote and archive)', async () => {
    const h = harness({
      left: local('/a'),
      right: remote('/b'),
      dirs: [...DIRS, 'sftp:b', 'sftp:b/src'],
    });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', local('/a/src'));
    expect(h.locations.right).toEqual(remote('/b/src'));

    const arc = harness({
      left: local('/a'),
      right: archive('/tmp/z.zip', ''),
      dirs: [...DIRS, 'archive:src'],
    });
    arc.sync.setEnabled(true);
    await arc.sync.navigation.navigate('left', local('/a/src'));
    expect(arc.locations.right).toEqual(archive('/tmp/z.zip', 'src'));
  });

  it('leaves non-hierarchical locations alone', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await h.sync.navigation.navigate('left', { providerId: 'search', uri: 'search://local/x' });
    expect(h.locations.right).toEqual(local('/b'));
    expect(h.sync.state()).toBe('desynced');
  });

  it('persists the per-workspace toggle and restores it', () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    expect(h.sync.state()).toBe('off');
    h.sync.toggle();
    expect(h.sync.state()).toBe('synced');
    expect(h.store.load('ws')).toBe(true);
    expect(h.onChange).toHaveBeenCalled();
    h.sync.toggle();
    expect(h.store.load('ws')).toBe(false);

    const restored = harness({ left: local('/a'), right: local('/b'), dirs: DIRS, stored: true });
    expect(restored.sync.state()).toBe('synced');
  });

  it('interleaved moves in both panes stay consistent', async () => {
    const h = harness({ left: local('/a'), right: local('/b'), dirs: DIRS });
    h.sync.setEnabled(true);
    await Promise.all([
      h.sync.navigation.navigate('left', local('/a/src')),
      h.sync.navigation.navigate('left', local('/a/src/deep')),
    ]);
    expect(h.locations.right?.uri).toBe('file:///b/src/deep');
  });
});

describe('local storage store', () => {
  it('round-trips and swallows storage failures', () => {
    const data = new Map<string, string>();
    const store = createLocalStorageSyncStore({
      getItem: (k) => data.get(k) ?? null,
      setItem: (k, v) => void data.set(k, v),
    });
    expect(store.load('w')).toBe(false);
    store.save('w', true);
    expect(store.load('w')).toBe(true);
    const broken = createLocalStorageSyncStore({
      getItem: () => {
        throw new Error('x');
      },
      setItem: () => {
        throw new Error('x');
      },
    });
    expect(broken.load('w')).toBe(false);
    expect(() => broken.save('w', true)).not.toThrow();
  });
});
