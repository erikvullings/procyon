import { describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import { entryIconRegistry, restoreDefaultIconTheme } from './entry-icons';
import { NativeIconLoader } from './native-icon-loader';

function entry(name: string, extension: string): EntrySummary {
  return {
    id: name,
    location: { providerId: 'file', uri: `file:///tmp/${name}` },
    name,
    kind: 'file',
    size: 1,
    modifiedAt: '2026-08-04T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension,
    metadataRevision: 1,
  };
}

describe('NativeIconLoader', () => {
  it('loads lazily and caches interleaved entries by normalized extension', async () => {
    const getFileIcon = vi
      .fn<FileManagerClient['getFileIcon']>()
      .mockResolvedValue(new Uint8Array([137, 80, 78, 71]));
    const redraw = vi.fn();
    const loader = new NativeIconLoader({ getFileIcon }, redraw);

    expect(loader.iconDataUri(entry('first.ABC', 'ABC'))).toBeUndefined();
    expect(loader.iconDataUri(entry('photo.unknown', 'unknown'))).toBeUndefined();
    expect(loader.iconDataUri(entry('second.abc', 'abc'))).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();

    expect(getFileIcon).toHaveBeenCalledTimes(2);
    expect(getFileIcon).toHaveBeenNthCalledWith(1, 'file:///tmp/first.ABC');
    expect(getFileIcon).toHaveBeenNthCalledWith(2, 'file:///tmp/photo.unknown');
    expect(loader.iconDataUri(entry('third.abc', 'abc'))).toBe('data:image/png;base64,iVBORw==');
    await vi.waitFor(() => expect(redraw).toHaveBeenCalledTimes(2));
  });

  it('does not fetch a native icon when the theme has an extension-specific icon', () => {
    const getFileIcon = vi.fn<FileManagerClient['getFileIcon']>();
    const loader = new NativeIconLoader({ getFileIcon }, vi.fn());

    expect(loader.iconDataUri(entry('document.pdf', 'pdf'))).toBeUndefined();
    expect(getFileIcon).not.toHaveBeenCalled();
  });

  it('does not fetch a native icon when the theme overrides the generic folder icon', () => {
    const getFileIcon = vi.fn<FileManagerClient['getFileIcon']>();
    const loader = new NativeIconLoader({ getFileIcon }, vi.fn());
    entryIconRegistry.kindIcons.set('directory', () => 'themed-folder');
    const folder = { ...entry('folder', ''), kind: 'directory' as const };

    try {
      expect(loader.iconDataUri(folder)).toBeUndefined();
      expect(getFileIcon).not.toHaveBeenCalled();
    } finally {
      restoreDefaultIconTheme();
    }
  });

  it('caches an unavailable icon and keeps returning the themed fallback path', async () => {
    const getFileIcon = vi.fn<FileManagerClient['getFileIcon']>().mockResolvedValue(undefined);
    const loader = new NativeIconLoader({ getFileIcon }, vi.fn());
    const textFile = entry('notes.unknown', 'unknown');

    expect(loader.iconDataUri(textFile)).toBeUndefined();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader.iconDataUri(textFile)).toBeUndefined();
    expect(getFileIcon).toHaveBeenCalledTimes(1);
  });
});
