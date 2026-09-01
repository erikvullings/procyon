import { describe, expect, it, vi } from 'vitest';

import type { EntrySummary } from '../../models';
import { VIDEO_EXTENSIONS } from '../directory-table/entry-icons';
import {
  audioMimeTypeFor,
  bytesToDataUri,
  IMAGE_RANGE_CHUNK_BYTES,
  imageMimeTypeFor,
  PREVIEW_SIZE_LIMIT_BYTES,
  readFullAudioDataUri,
  readFullImageDataUri,
  resolvePreviewKind,
  videoMimeTypeFor,
} from './content-preview';

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'entry-1',
    location: { providerId: 'local', uri: 'file:///tmp/report.txt' },
    name: 'report.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

describe('resolvePreviewKind', () => {
  it('resolves directories and symlinks to metadata', () => {
    expect(resolvePreviewKind(entry({ kind: 'directory' }))).toBe('metadata');
    expect(resolvePreviewKind(entry({ kind: 'symlink' }))).toBe('metadata');
  });

  it('resolves an image extension to image', () => {
    expect(resolvePreviewKind(entry({ name: 'photo.png', extension: 'png' }))).toBe('image');
    expect(resolvePreviewKind(entry({ name: 'photo.JPEG', extension: 'JPEG' }))).toBe('image');
    expect(resolvePreviewKind(entry({ name: 'favicon.ico', extension: 'ico' }))).toBe('image');
  });

  it('resolves an audio extension to audio', () => {
    expect(resolvePreviewKind(entry({ name: 'song.mp3', extension: 'mp3' }))).toBe('audio');
    expect(resolvePreviewKind(entry({ name: 'song.FLAC', extension: 'FLAC' }))).toBe('audio');
  });

  it.each(VIDEO_EXTENSIONS)('resolves the .%s video extension to video', (extension) => {
    expect(resolvePreviewKind(entry({ name: `clip.${extension}`, extension }))).toBe('video');
  });

  it('resolves any other file extension to text', () => {
    expect(resolvePreviewKind(entry({ name: 'report.txt', extension: 'txt' }))).toBe('text');
  });

  it.each(['zip', 'tar', 'tar.gz', 'tgz', '7z', 'rar', 'gz', 'tar.bz2', 'tar.xz'])(
    'resolves a non-comic .%s archive to an archive summary',
    (extension) => {
      expect(
        resolvePreviewKind(
          entry({
            name: `archive.${extension}`,
            extension: extension.split('.').at(-1) ?? extension,
          }),
        ),
      ).toBe('archiveSummary');
    },
  );

  it('keeps comic and EPUB archives on their dedicated renderers', () => {
    expect(resolvePreviewKind(entry({ name: 'comic.cbz', extension: 'cbz' }))).toBe('comic');
    expect(resolvePreviewKind(entry({ name: 'comic.cbr', extension: 'cbr' }))).toBe('comic');
    expect(resolvePreviewKind(entry({ name: 'book.epub', extension: 'epub' }))).toBe('epub');
    expect(resolvePreviewKind(entry({ name: 'report.docx', extension: 'docx' }))).toBe('docx');
    expect(resolvePreviewKind(entry({ name: 'briefing.pptx', extension: 'pptx' }))).toBe('pptx');
  });
});

describe('imageMimeTypeFor', () => {
  it('prefers a known extension over the reported mimeType', () => {
    expect(
      imageMimeTypeFor(entry({ extension: 'png', mimeType: 'application/octet-stream' })),
    ).toBe('image/png');
    expect(
      imageMimeTypeFor(entry({ extension: 'ico', mimeType: 'application/octet-stream' })),
    ).toBe('image/x-icon');
  });

  it('falls back to the reported mimeType for an unknown extension', () => {
    expect(imageMimeTypeFor(entry({ extension: 'xyz', mimeType: 'image/x-custom' }))).toBe(
      'image/x-custom',
    );
  });

  it('falls back to a generic type when neither is known', () => {
    expect(imageMimeTypeFor(entry())).toBe('application/octet-stream');
  });
});

describe('audioMimeTypeFor', () => {
  it('prefers a known extension over the reported mimeType', () => {
    expect(
      audioMimeTypeFor(entry({ extension: 'mp3', mimeType: 'application/octet-stream' })),
    ).toBe('audio/mpeg');
  });

  describe('videoMimeTypeFor', () => {
    it.each([
      ['mp4', 'video/mp4'],
      ['m4v', 'video/mp4'],
      ['mov', 'video/quicktime'],
      ['webm', 'video/webm'],
      ['avi', 'video/x-msvideo'],
    ])('maps .%s to %s', (extension, mimeType) => {
      expect(videoMimeTypeFor(entry({ extension, mimeType: 'application/octet-stream' }))).toBe(
        mimeType,
      );
    });
  });

  it('falls back to the reported mimeType for an unknown extension', () => {
    expect(audioMimeTypeFor(entry({ extension: 'xyz', mimeType: 'audio/x-custom' }))).toBe(
      'audio/x-custom',
    );
  });

  it('falls back to a generic type when neither is known', () => {
    expect(audioMimeTypeFor(entry())).toBe('application/octet-stream');
  });
});

describe('bytesToDataUri', () => {
  it('encodes bytes as a base64 data URI', () => {
    const uri = bytesToDataUri(new Uint8Array([72, 101, 108, 108, 111]), 'text/plain');
    expect(uri).toBe(`data:text/plain;base64,${btoa('Hello')}`);
  });
});

describe('PREVIEW_SIZE_LIMIT_BYTES', () => {
  it('is a positive, sensibly small default', () => {
    expect(PREVIEW_SIZE_LIMIT_BYTES).toBeGreaterThan(0);
    expect(PREVIEW_SIZE_LIMIT_BYTES).toBeLessThanOrEqual(16 * 1024 * 1024);
  });
});

describe('readFullImageDataUri', () => {
  it('does not overflow the call stack on a full-size (1 MiB) chunk', async () => {
    // Regression test: a naive `chunks.push(...chunk.data)` spreads ~1,048,576 arguments into a
    // single call and throws "Maximum call stack size exceeded".
    const bytes = new Array<number>(IMAGE_RANGE_CHUNK_BYTES).fill(1);
    const readFileRange = vi.fn().mockResolvedValue({
      data: bytes,
      eof: true,
      length: bytes.length,
      offset: 0,
    });
    const entryValue = entry({ name: 'photo.png', extension: 'png' });
    const uri = await readFullImageDataUri(
      { readFileRange },
      entryValue,
      new AbortController().signal,
    );
    expect(uri.startsWith('data:image/png;base64,')).toBe(true);
  });

  it('concatenates multiple chunks in order', async () => {
    const readFileRange = vi
      .fn()
      .mockResolvedValueOnce({ data: [72, 101], eof: false, length: 2, offset: 0 })
      .mockResolvedValueOnce({ data: [108, 108, 111], eof: true, length: 3, offset: 2 });
    const uri = await readFullImageDataUri(
      { readFileRange },
      entry({ name: 'photo.png', extension: 'png' }),
      new AbortController().signal,
    );
    expect(uri).toBe(`data:image/png;base64,${btoa('Hello')}`);
  });
});

describe('readFullAudioDataUri', () => {
  it('concatenates multiple chunks and encodes with the audio MIME type', async () => {
    const readFileRange = vi
      .fn()
      .mockResolvedValueOnce({ data: [72, 101], eof: false, length: 2, offset: 0 })
      .mockResolvedValueOnce({ data: [108, 108, 111], eof: true, length: 3, offset: 2 });
    const uri = await readFullAudioDataUri(
      { readFileRange },
      entry({ name: 'song.mp3', extension: 'mp3' }),
      new AbortController().signal,
    );
    expect(uri).toBe(`data:audio/mpeg;base64,${btoa('Hello')}`);
  });
});
