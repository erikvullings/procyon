import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import {
  addToBasket,
  basketSources,
  classifyBasketAbsence,
  emptyBasket,
  loadBasket,
  refreshBasket,
  removeFromBasket,
  saveBasket,
  selectBasketItems,
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

  it('removes a single item without touching the other provider', () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://server/b'),
    ]);
    expect(removeFromBasket(basket, basket.items[0]!.key).items).toHaveLength(1);
  });

  it('runs only the checked subset, but runs all when none are checked', () => {
    const basket = addToBasket(emptyBasket, [
      entry('a', 'local', 'file:///a'),
      entry('b', 'sftp', 'sftp://server/b'),
    ]);
    expect(basketSources(selectBasketItems(basket, [basket.items[1]!.key]))).toEqual([
      basket.items[1]!.location,
    ]);
    expect(basketSources(basket)).toHaveLength(2);
  });

  it('restores only an opted-in workspace basket and marks references stale until rechecked', () => {
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
    const persistent = {
      ...addToBasket(emptyBasket, [entry('a', 'local', 'file:///a')]),
      persist: true,
    };
    saveBasket(store, 'workspace-a', persistent);
    expect(loadBasket(store, 'workspace-a').items[0]?.status).toBe('stale');
    expect(loadBasket(store, 'workspace-b')).toEqual(emptyBasket);
    saveBasket(store, 'workspace-a', { ...persistent, persist: false });
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
    const checked = await refreshBasket(basket, async (item) =>
      item.id === 'b' ? 'missing' : item.id === 'c' ? 'stale' : 'ready',
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
