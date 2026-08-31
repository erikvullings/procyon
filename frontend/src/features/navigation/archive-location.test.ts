import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models/entry';
import { archiveEntryLocation, archiveRootForEntry } from './archive-location';

function entry(name: string, uri = `file:///tmp/${name}`): EntrySummary {
  return {
    id: name,
    location: { providerId: 'local', uri },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
  };
}

describe('archiveRootForEntry', () => {
  it.each([
    'photos.zip',
    'backup.7z',
    'old.RAR',
    'comic.cbz',
    'comic.cbr',
    'files.tar',
    'files.tar.gz',
    'files.tbz2',
    'document.txt.gz',
    'book.epub',
  ])('maps %s to its folder-like archive root', (name) => {
    expect(archiveRootForEntry(entry(name))).toEqual({
      providerId: 'archive',
      uri: `archive:///tmp/${name}!/`,
    });
  });

  it('leaves ordinary files and non-local entries to the normal open action', () => {
    expect(archiveRootForEntry(entry('notes.txt'))).toBeUndefined();
    expect(
      archiveRootForEntry({
        ...entry('nested.zip'),
        location: { providerId: 'archive', uri: 'archive:///tmp/outer.zip!/nested.zip' },
      }),
    ).toBeUndefined();
  });
});

describe('archiveEntryLocation', () => {
  it('appends the inner path to the archive root', () => {
    const root = archiveRootForEntry(entry('book.epub'));
    expect(root).toBeDefined();
    expect(
      archiveEntryLocation(root as NonNullable<typeof root>, 'META-INF/container.xml'),
    ).toEqual({
      providerId: 'archive',
      uri: 'archive:///tmp/book.epub!/META-INF/container.xml',
    });
  });
});
