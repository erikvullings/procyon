import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import { ThumbnailLoader } from './thumbnail-loader';

function entry(name: string, extension: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  return {
    id: name,
    location: { providerId: 'file', uri: `file:///tmp/${name}` },
    name,
    kind,
    size: 1,
    modifiedAt: '2026-08-04T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension,
    metadataRevision: 1,
  };
}

function noopReadFileRange(): ReturnType<FileManagerClient['readFileRange']> {
  return Promise.reject(new Error('readFileRange not stubbed for this test'));
}

describe('ThumbnailLoader', () => {
  it('loads lazily and caches interleaved entries by uri and size', async () => {
    const getThumbnail = vi
      .fn<FileManagerClient['getThumbnail']>()
      .mockResolvedValue(new Uint8Array([0xff, 0xd8, 0xff, 0xe0]));
    const redraw = vi.fn();
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, redraw);

    expect(loader.thumbnailDataUri(entry('first.png', 'png'), 'small')).toBeUndefined();
    expect(loader.thumbnailDataUri(entry('second.jpg', 'jpg'), 'small')).toBeUndefined();
    // Same file requested again before the first fetch resolves - must dedup, not refetch.
    expect(loader.thumbnailDataUri(entry('first.png', 'png'), 'small')).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();

    expect(getThumbnail).toHaveBeenCalledTimes(2);
    expect(getThumbnail).toHaveBeenNthCalledWith(
      1,
      'file:///tmp/first.png',
      'small',
      expect.any(AbortSignal),
    );
    expect(getThumbnail).toHaveBeenNthCalledWith(
      2,
      'file:///tmp/second.jpg',
      'small',
      expect.any(AbortSignal),
    );
    expect(loader.thumbnailDataUri(entry('first.png', 'png'), 'small')).toBe(
      'data:image/jpeg;base64,/9j/4A==',
    );
    await vi.waitFor(() => expect(redraw).toHaveBeenCalledTimes(2));
  });

  it('fetches the same file again for a different requested size', async () => {
    const getThumbnail = vi
      .fn<FileManagerClient['getThumbnail']>()
      .mockResolvedValue(new Uint8Array([1, 2, 3, 4]));
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());
    const file = entry('photo.png', 'png');

    loader.thumbnailDataUri(file, 'small');
    loader.thumbnailDataUri(file, 'large');
    await Promise.resolve();
    await Promise.resolve();

    expect(getThumbnail).toHaveBeenCalledTimes(2);
    expect(getThumbnail).toHaveBeenNthCalledWith(
      1,
      'file:///tmp/photo.png',
      'small',
      expect.any(AbortSignal),
    );
    expect(getThumbnail).toHaveBeenNthCalledWith(
      2,
      'file:///tmp/photo.png',
      'large',
      expect.any(AbortSignal),
    );
  });

  it('does not fetch for a directory or an unsupported extension', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>();
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());

    expect(loader.thumbnailDataUri(entry('folder', '', 'directory'), 'small')).toBeUndefined();
    expect(loader.thumbnailDataUri(entry('notes.txt', 'txt'), 'small')).toBeUndefined();
    expect(getThumbnail).not.toHaveBeenCalled();
  });

  it('caches an unavailable thumbnail and keeps returning the icon fallback path', async () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>().mockResolvedValue(undefined);
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());
    const file = entry('broken.png', 'png');

    expect(loader.thumbnailDataUri(file, 'small')).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.thumbnailDataUri(file, 'small')).toBeUndefined();
    expect(getThumbnail).toHaveBeenCalledTimes(1);
  });

  it('resolves to undefined if the client rejects, instead of throwing', async () => {
    const getThumbnail = vi
      .fn<FileManagerClient['getThumbnail']>()
      .mockRejectedValue(new Error('boom'));
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());
    const file = entry('flaky.png', 'png');

    expect(loader.thumbnailDataUri(file, 'small')).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.thumbnailDataUri(file, 'small')).toBeUndefined();
  });

  it('treats cbz and cbr archives as thumbnailable', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>().mockResolvedValue(undefined);
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());

    loader.thumbnailDataUri(entry('issue.cbz', 'cbz'), 'medium');
    loader.thumbnailDataUri(entry('issue.cbr', 'cbr'), 'medium');

    expect(getThumbnail).toHaveBeenCalledTimes(2);
  });

  it('treats mp4/m4v/mov video and pdf as thumbnailable', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>().mockResolvedValue(undefined);
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());

    loader.thumbnailDataUri(entry('clip.mp4', 'mp4'), 'medium');
    loader.thumbnailDataUri(entry('clip.m4v', 'm4v'), 'medium');
    loader.thumbnailDataUri(entry('clip.mov', 'mov'), 'medium');
    loader.thumbnailDataUri(entry('document.pdf', 'pdf'), 'medium');

    expect(getThumbnail).toHaveBeenCalledTimes(4);
  });

  it('renders an svg by reading its raw markup, never through the JPEG thumbnail endpoint', async () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>();
    const readFileRange = vi.fn<FileManagerClient['readFileRange']>().mockResolvedValue({
      data: [60, 115, 118, 103, 62, 60, 47, 115, 118, 103, 62], // "<svg></svg>"
      eof: true,
      length: 11,
      offset: 0,
    });
    const redraw = vi.fn();
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange }, redraw);
    const file = entry('icon.svg', 'svg');

    expect(loader.thumbnailDataUri(file, 'small')).toBeUndefined();
    await vi.waitFor(() => expect(redraw).toHaveBeenCalledTimes(1));

    expect(getThumbnail).not.toHaveBeenCalled();
    expect(readFileRange).toHaveBeenCalledTimes(1);
    expect(readFileRange).toHaveBeenCalledWith(
      {
        location: file.location,
        offset: 0,
        length: 512 * 1024,
      },
      expect.any(AbortSignal),
    );
    expect(loader.thumbnailDataUri(file, 'small')).toBe(
      'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=',
    );
    // A different requested size reuses the same cached svg content instead of refetching.
    expect(loader.thumbnailDataUri(file, 'large')).toBe(
      'data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=',
    );
    expect(readFileRange).toHaveBeenCalledTimes(1);
  });

  it('skips an svg thumbnail when the file is over the size limit or the read is truncated', async () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>();
    const readFileRange = vi
      .fn<FileManagerClient['readFileRange']>()
      .mockResolvedValue({ data: [], eof: false, length: 3 * 1024, offset: 0 });
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange }, vi.fn());

    const oversized = entry('huge.svg', 'svg');
    oversized.size = 3 * 1024 + 1;
    expect(loader.thumbnailDataUri(oversized, 'small')).toBeUndefined();
    expect(readFileRange).not.toHaveBeenCalled();

    const truncated = entry('truncated.svg', 'svg');
    expect(loader.thumbnailDataUri(truncated, 'small')).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.thumbnailDataUri(truncated, 'small')).toBeUndefined();
  });

  it('does not read an oversized svg even at grid thumbnail sizes', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>();
    const readFileRange = vi.fn<FileManagerClient['readFileRange']>();
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange }, vi.fn());
    const oversized = entry('huge.svg', 'svg');
    oversized.size = 512 * 1024 + 1;

    expect(loader.thumbnailDataUri(oversized, 'medium')).toBeUndefined();
    expect(readFileRange).not.toHaveBeenCalled();
  });

  it('skips large raster images in list rows but loads them for the thumbnail grid', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>().mockResolvedValue(undefined);
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());

    const bigPng = entry('photo.png', 'png');
    bigPng.size = 3 * 1024 + 1;

    expect(loader.thumbnailDataUri(bigPng, 'small')).toBeUndefined();
    loader.thumbnailDataUri(bigPng, 'medium');

    expect(getThumbnail).toHaveBeenCalledTimes(1);
    expect(getThumbnail).toHaveBeenCalledWith(
      'file:///tmp/photo.png',
      'medium',
      expect.any(AbortSignal),
    );
  });

  it('always thumbnails an ico file, unlike other images, regardless of size', () => {
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>().mockResolvedValue(undefined);
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());

    const bigIco = entry('huge.ico', 'ico');
    bigIco.size = 10 * 1024 * 1024;

    loader.thumbnailDataUri(bigIco, 'small');

    expect(getThumbnail).toHaveBeenCalledTimes(1);
    expect(getThumbnail).toHaveBeenCalledWith(
      'file:///tmp/huge.ico',
      'small',
      expect.any(AbortSignal),
    );
  });

  it('cancels queued and active viewport requests after their tiles leave the viewport', async () => {
    const requests: Array<{ signal: AbortSignal; resolve: () => void }> = [];
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>(
      (_uri, _size, signal) =>
        new Promise((resolve, reject) => {
          if (signal === undefined) throw new Error('missing abort signal');
          signal.addEventListener('abort', () => reject(signal.reason), { once: true });
          requests.push({ signal, resolve: () => resolve(undefined) });
        }),
    );
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());
    const viewport = loader.createViewport();

    viewport.beginFrame();
    for (let index = 0; index < 6; index += 1) {
      viewport.thumbnailDataUri(entry(`photo-${index}.png`, 'png'), 'medium');
    }
    viewport.endFrame();
    expect(getThumbnail).toHaveBeenCalledTimes(4);

    viewport.beginFrame();
    viewport.thumbnailDataUri(entry('photo-5.png', 'png'), 'medium');
    viewport.endFrame();

    expect(requests.every(({ signal }) => signal.aborted)).toBe(true);
    await vi.waitFor(() => expect(getThumbnail).toHaveBeenCalledTimes(5));
    expect(getThumbnail.mock.calls[4]?.[0]).toBe('file:///tmp/photo-5.png');

    requests[4]?.resolve();
    viewport.dispose();
  });

  it('keeps a shared request alive when only its grid tile leaves the viewport', () => {
    let signal: AbortSignal | undefined;
    const getThumbnail = vi.fn<FileManagerClient['getThumbnail']>(
      (_uri, _size, requestSignal) =>
        new Promise(() => {
          signal = requestSignal;
        }),
    );
    const loader = new ThumbnailLoader({ getThumbnail, readFileRange: noopReadFileRange }, vi.fn());
    const viewport = loader.createViewport();
    const photo = entry('shared.png', 'png');

    viewport.beginFrame();
    viewport.thumbnailDataUri(photo, 'large');
    viewport.endFrame();
    loader.thumbnailDataUri(photo, 'large');

    viewport.beginFrame();
    viewport.endFrame();

    expect(signal?.aborted).toBe(false);
    viewport.dispose();
  });
});
