import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import type { ListDirectoryRequest } from '../../models/requests';
import {
  collectMedia,
  hasPlayableSelection,
  isPlayableMedia,
  queueItemsFromEntries,
} from './play-selection';

function entry(path: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  const name = path.split('/').pop() ?? path;
  const dot = name.lastIndexOf('.');
  return {
    id: `id:${path}`,
    location: { providerId: 'local', uri: `file://${path}` },
    name,
    kind,
    ...(dot > 0 ? { extension: name.slice(dot + 1) } : {}),
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

const tree: Record<string, EntrySummary[]> = {
  '/music': [
    entry('/music/sub', 'directory'),
    entry('/music/b.mp3'),
    entry('/music/readme.txt'),
    entry('/music/a.flac'),
  ],
  '/music/sub': [entry('/music/sub/deep.ogg'), entry('/music/sub/cover.jpg')],
};

function fakeClient() {
  const listDirectory = vi.fn((request: ListDirectoryRequest) => {
    const path = request.location.uri.replace('file://', '');
    return Promise.resolve({
      paneId: request.paneId,
      requestId: request.requestId,
      revision: 1,
      location: request.location,
      writable: true,
      entries: tree[path] ?? [],
      hasMore: false,
      loadingState: { type: 'loaded' as const },
    });
  });
  return { client: { listDirectory }, listDirectory };
}

function options(recursive: boolean) {
  const { client, listDirectory } = fakeClient();
  return {
    listDirectory,
    options: {
      client,
      workspaceId: 'w1',
      view: {
        sort: [{ columnId: 'core.name', direction: 'ascending' as const }],
        showHidden: false,
      },
      recursive,
    },
  };
}

describe('collectMedia', () => {
  it('keeps selected files in order, ignores non-media and lists a folder non-recursively', async () => {
    const { options: opts, listDirectory } = options(false);
    const result = await collectMedia(
      [entry('/x/first.wav'), entry('/x/notes.txt'), entry('/music', 'directory')],
      opts,
    );
    expect(result.entries.map((e) => e.name)).toEqual(['first.wav', 'b.mp3', 'a.flac']);
    expect(listDirectory).toHaveBeenCalledTimes(1);
    expect(listDirectory.mock.calls[0]?.[0]).toMatchObject({
      sort: [{ columnId: 'core.name', direction: 'ascending' }],
      showHidden: false,
    });
  });

  it('descends into subfolders in listing order when recursive', async () => {
    const { options: opts } = options(true);
    const result = await collectMedia([entry('/music', 'directory')], opts);
    expect(result.entries.map((e) => e.name)).toEqual(['deep.ogg', 'b.mp3', 'a.flac']);
  });

  it('never follows symlinks and de-duplicates repeated files', async () => {
    const { options: opts, listDirectory } = options(true);
    const result = await collectMedia(
      [entry('/link', 'symlink'), entry('/x/a.mp3'), entry('/x/a.mp3')],
      opts,
    );
    expect(result.entries).toHaveLength(1);
    expect(listDirectory).not.toHaveBeenCalled();
  });

  it('stops quietly when aborted', async () => {
    const controller = new AbortController();
    controller.abort();
    const { options: opts } = options(true);
    const result = await collectMedia([entry('/x/a.mp3')], { ...opts, signal: controller.signal });
    expect(result.entries).toEqual([]);
  });
});

describe('selection helpers', () => {
  it('recognises media files and playable selections', () => {
    expect(isPlayableMedia(entry('/a.mp3'))).toBe(true);
    expect(isPlayableMedia(entry('/a.txt'))).toBe(false);
    expect(isPlayableMedia(entry('/a.mp3', 'directory'))).toBe(false);
    expect(hasPlayableSelection([entry('/a.txt')])).toBe(false);
    expect(hasPlayableSelection([entry('/a.txt'), entry('/dir', 'directory')])).toBe(true);
  });

  it('titles queue items by file name without the extension', () => {
    const [item] = queueItemsFromEntries([entry('/m/My Song.mp3')]);
    expect(item?.title).toBe('My Song');
  });
});
