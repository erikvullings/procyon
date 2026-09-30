import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import {
  addToBasket,
  basketSources,
  basketSummary,
  classifyBasketAbsence,
  emptyBasket,
  findBasketOverlap,
  loadBasket,
  refreshBasket,
  removeFromBasket,
  saveBasket,
  selectBasketItems,
  withBasketFolderSize,
  withBasketStatus,
} from './basket';

const entry = (id: string, providerId: string, uri: string): EntrySummary => ({
  id,
  location: { providerId, uri },
  name: uri.split('/').at(-1) ?? uri,
  kind: 'file',
  hidden: false,
  readOnly: false,
  metadataRevision: 0,
});

describe('collection basket', () => {
  it('collects references from mixed providers and does not duplicate a stable entry', () => {
    const local = entry('local-1', 'local', 'file:///docs/a.txt');
    const remote = entry('remote-1', 'sftp', 'sftp://server/docs/b.txt');
    const basket = addToBasket(addToBasket(emptyBasket, [local, remote]), [local]);
    expect(basket.items.map(({ id, location }) => [id, location])).toEqual([
      ['local-1', local.location],
      ['remote-1', remote.location],
    ]);
  });

  it('replaces collected children with a newly added parent folder', () => {
    const folder = { ...entry('folder', 'local', 'file:///docs'), kind: 'directory' as const };
    const child = entry('child', 'local', 'file:///docs/sub/report.txt');
    const basket = addToBasket(emptyBasket, [
      child,
      entry('another-child', 'local', 'file:///docs/other.txt'),
      entry('sibling', 'local', 'file:///docs-other/report.txt'),
    ]);
    const selected = selectBasketItems(basket, [basket.items[0]?.key ?? '']);
    const replaced = addToBasket(selected, [folder]);
    expect(replaced.items.map((item) => item.id)).toEqual(['sibling', 'folder']);
    expect(replaced.selectedKeys).toEqual([]);
    expect(addToBasket(emptyBasket, [child, folder]).items.map((item) => item.id)).toEqual([
      'folder',
    ]);
  });

  it('prevents adding a child of a collected folder across providers and path prefixes', () => {
    const folder = { ...entry('folder', 'local', 'file:///docs'), kind: 'directory' as const };
    const child = entry('child', 'local', 'file:///docs/sub/report.txt');
    const basket = addToBasket(emptyBasket, [
      folder,
      entry('sibling', 'local', 'file:///docs-other/report.txt'),
      entry('remote', 'sftp', 'sftp://server/docs/sub/report.txt'),
    ]);
    expect(() => addToBasket(basket, [child])).toThrow();
    expect(() => addToBasket(emptyBasket, [folder, child])).toThrow();
    expect(basket.items).toHaveLength(3);
    expect(findBasketOverlap(basket.items)).toBeUndefined();
    expect(
      addToBasket(emptyBasket, [
        folder,
        entry('another-host', 'sftp', 'sftp://other/docs/sub/report.txt'),
      ]).items,
    ).toHaveLength(2);
    const remoteFolder = {
      ...entry('remote-folder', 'sftp', 'sftp://server/docs'),
      kind: 'directory' as const,
    };
    expect(
      addToBasket(addToBasket(emptyBasket, [remoteFolder]), [
        entry('other-host-child', 'sftp', 'sftp://other/docs/report.txt'),
      ]).items,
    ).toHaveLength(2);
    const root = { ...entry('root', 'local', 'file:///'), kind: 'directory' as const };
    expect(findBasketOverlap(addToBasket(emptyBasket, [root]).items)).toBeUndefined();
    expect(() => addToBasket(addToBasket(emptyBasket, [root]), [folder])).toThrow();
  });

  it('detects selected overlaps in previously persisted baskets', () => {
    const folder = addToBasket(emptyBasket, [
      { ...entry('folder', 'local', 'file:///docs'), kind: 'directory' },
    ]).items[0];
    const child = addToBasket(emptyBasket, [entry('child', 'local', 'file:///docs/report.txt')])
      .items[0];
    expect(folder).toBeDefined();
    expect(child).toBeDefined();
    if (folder === undefined || child === undefined) return;
    expect(findBasketOverlap([folder, child])).toEqual([folder, child]);
    expect(findBasketOverlap([child])).toBeUndefined();
    const root = { ...entry('root', 'local', 'file:///'), kind: 'directory' as const };
    expect(
      addToBasket({ ...emptyBasket, items: [folder, child], selectedKeys: [child.key] }, [root]),
    ).toEqual({ ...emptyBasket, items: [expect.objectContaining({ id: 'root' })] });
  });

  it('removes a single item without touching the other provider', () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://server/b'),
    ]);
    expect(removeFromBasket(basket, basket.items[0]!.key).items).toHaveLength(1);
  });

  it('runs only checked entries and none when no entries are checked', () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://server/b'),
    ]);
    expect(basketSources(selectBasketItems(basket, [basket.items[1]!.key]))).toEqual([
      basket.items[1]!.location,
    ]);
    expect(basketSources(basket)).toEqual([]);
  });

  it('summarizes mixed entries and counts only available, known file sizes', () => {
    const basket = addToBasket(emptyBasket, [
      { ...entry('a', 'local', 'file:///a'), size: 2_048 },
      { ...entry('b', 'sftp', 'sftp://server/b'), size: 1_024 },
      { ...entry('c', 'local', 'file:///c'), kind: 'directory' as const },
      entry('d', 'local', 'file:///d'),
      { ...entry('e', 'local', 'file:///e'), size: 4_096 },
    ]);
    const selected = selectBasketItems(basket, [
      basket.items[1]?.key ?? '',
      basket.items[4]?.key ?? '',
    ]);
    const unavailable = withBasketStatus(selected, basket.items[4]?.key ?? '', 'missing');
    expect(basketSummary(unavailable)).toEqual({
      fileCount: 4,
      folderCount: 1,
      knownSize: 3_072,
      incompleteSize: true,
      unavailableCount: 1,
      selectedCount: 2,
      selectedKnownSize: 1_024,
    });
    expect(basketSummary(emptyBasket)).toEqual({
      fileCount: 0,
      folderCount: 0,
      knownSize: 0,
      incompleteSize: false,
      unavailableCount: 0,
      selectedCount: 0,
      selectedKnownSize: 0,
    });
  });

  it('includes calculated folder contents in total and selected sizes', () => {
    const basket = addToBasket(emptyBasket, [
      entry('file', 'local', 'file:///file.txt'),
      { ...entry('folder', 'local', 'file:///folder'), kind: 'directory' },
    ]);
    const measured = withBasketFolderSize(
      selectBasketItems(basket, [basket.items[1]?.key ?? '']),
      basket.items[1]?.key ?? '',
      8_192,
    );
    expect(basketSummary(measured)).toMatchObject({
      knownSize: 8_192,
      selectedKnownSize: 8_192,
      incompleteSize: true,
    });
    const complete = withBasketFolderSize(measured, basket.items[1]?.key ?? '', 0);
    expect(basketSummary(complete).selectedKnownSize).toBe(0);
  });

  it('persists each workspace basket automatically and marks restored references stale', () => {
    const storage = new Map<string, string>();
    const store = {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
      removeItem: (key: string) => {
        storage.delete(key);
      },
    };
    const persistent = addToBasket(emptyBasket, [entry('a', 'local', 'file:///a')]);
    saveBasket(store, 'workspace-a', persistent);
    expect(loadBasket(store, 'workspace-a').items[0]?.status).toBe('stale');
    expect(loadBasket(store, 'workspace-b')).toEqual(emptyBasket);
    saveBasket(store, 'workspace-a', emptyBasket);
    expect(loadBasket(store, 'workspace-a')).toEqual(emptyBasket);
  });

  it('rejects credential-bearing references instead of persisting them', () => {
    expect(() =>
      addToBasket(emptyBasket, [entry('a', 'sftp', 'sftp://user:secret@server/a')]),
    ).toThrow('credentials');
  });

  it('rechecks interleaved provider references before using them, excluding missing and stale entries', async () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://one/b'),
      entry('c', 'local', 'file:///c'),
      entry('d', 'sftp', 'sftp://two/d'),
    ]);
    const checked = await refreshBasket(
      selectBasketItems(
        basket,
        basket.items.map((item) => item.key),
      ),
      async (item) => (item.id === 'b' ? 'missing' : item.id === 'c' ? 'stale' : 'ready'),
    );
    expect(basketSources(checked)).toEqual([basket.items[0]!.location, basket.items[3]!.location]);
  });

  it('does not inspect unrelated entries when acting on a checked subset', async () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://offline/b'),
    ]);
    const selected = selectBasketItems(basket, [basket.items[0]!.key]);
    const checked = await refreshBasket(
      selected,
      async (item) => {
        if (item.id === 'b') throw new Error('provider offline');
        return 'ready';
      },
      new Set(selected.selectedKeys),
    );
    expect(basketSources(checked)).toEqual([basket.items[0]!.location]);
  });

  it('reports a moved entry when its stable identity is visible at a new location', () => {
    const collected = addToBasket(emptyBasket, [entry('same', 'local', 'file:///before')])
      .items[0]!;
    expect(
      classifyBasketAbsence(collected, [
        entry('same', 'sftp', 'sftp://server/before'),
        entry('different', 'local', 'file:///before'),
        entry('same', 'local', 'file:///after'),
      ]),
    ).toBe('moved');
    expect(classifyBasketAbsence(collected, [entry('same', 'sftp', 'sftp://server/before')])).toBe(
      'missing',
    );
  });
});
