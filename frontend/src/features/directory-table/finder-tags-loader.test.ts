import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import { FinderTagsLoader } from './finder-tags-loader';

function entry(name: string, id = name, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  return {
    id,
    location: { providerId: 'file', uri: `file:///tmp/${name}` },
    name,
    kind,
    size: 1,
    modifiedAt: '2026-08-17T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

describe('FinderTagsLoader', () => {
  it('loads lazily and caches interleaved entries by uri', async () => {
    const getFinderTags = vi
      .fn<FileManagerClient['getFinderTags']>()
      .mockResolvedValue({ tags: [{ name: 'Work', color: 'blue' }] });
    const redraw = vi.fn();
    const loader = new FinderTagsLoader({ getFinderTags }, redraw);

    expect(loader.finderTags(entry('first.txt'))).toBeUndefined();
    expect(loader.finderTags(entry('second.txt'))).toBeUndefined();
    // Same entry requested again before the first fetch resolves - must dedup, not refetch.
    expect(loader.finderTags(entry('first.txt'))).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();

    expect(getFinderTags).toHaveBeenCalledTimes(2);
    expect(getFinderTags).toHaveBeenNthCalledWith(1, 'file:///tmp/first.txt');
    expect(getFinderTags).toHaveBeenNthCalledWith(2, 'file:///tmp/second.txt');
    expect(loader.finderTags(entry('first.txt'))).toEqual({
      tags: [{ name: 'Work', color: 'blue' }],
    });
    await vi.waitFor(() => expect(redraw).toHaveBeenCalledTimes(2));
  });

  it('applies to directories as well as files', async () => {
    const getFinderTags = vi
      .fn<FileManagerClient['getFinderTags']>()
      .mockResolvedValue({ tags: [] });
    const loader = new FinderTagsLoader({ getFinderTags }, vi.fn());

    loader.finderTags(entry('folder', 'folder', 'directory'));
    await Promise.resolve();

    expect(getFinderTags).toHaveBeenCalledWith('file:///tmp/folder');
  });

  it('never fetches for a synthetic parent-navigation entry', () => {
    const getFinderTags = vi.fn<FileManagerClient['getFinderTags']>();
    const loader = new FinderTagsLoader({ getFinderTags }, vi.fn());

    expect(loader.finderTags(entry('..', 'fm:parent:file:///tmp'))).toBeUndefined();
    expect(getFinderTags).not.toHaveBeenCalled();
  });

  it('caches an unsupported/failed lookup and keeps returning no badge', async () => {
    const getFinderTags = vi.fn<FileManagerClient['getFinderTags']>().mockResolvedValue(undefined);
    const loader = new FinderTagsLoader({ getFinderTags }, vi.fn());
    const file = entry('untagged.txt');

    expect(loader.finderTags(file)).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.finderTags(file)).toBeUndefined();
    expect(getFinderTags).toHaveBeenCalledTimes(1);
  });

  it('resolves to undefined if the client rejects, instead of throwing', async () => {
    const getFinderTags = vi
      .fn<FileManagerClient['getFinderTags']>()
      .mockRejectedValue(new Error('boom'));
    const loader = new FinderTagsLoader({ getFinderTags }, vi.fn());
    const file = entry('flaky.txt');

    expect(loader.finderTags(file)).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.finderTags(file)).toBeUndefined();
  });

  it('setCached seeds the cache immediately after a successful edit, without a refetch', async () => {
    const getFinderTags = vi.fn<FileManagerClient['getFinderTags']>();
    const redraw = vi.fn();
    const loader = new FinderTagsLoader({ getFinderTags }, redraw);
    const uri = 'file:///tmp/edited.txt';

    loader.setCached(uri, { tags: [{ name: 'Red', color: 'red' }] });

    expect(loader.finderTags(entry('edited.txt'))).toEqual({
      tags: [{ name: 'Red', color: 'red' }],
    });
    expect(getFinderTags).not.toHaveBeenCalled();
    expect(redraw).toHaveBeenCalledTimes(1);
  });
});
