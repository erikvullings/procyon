import { describe, expect, it } from 'vitest';

import type { Location } from '../../models';
import { DEFAULT_DOCUMENT_HUB_SETTINGS } from '../../models/settings';
import { documentHubQuery, normalizeHubExtensions } from './document-hub';

const HOME: Location = { providerId: 'local', uri: 'file:///Users/me' };

describe('normalizeHubExtensions', () => {
  it('lower-cases, trims, strips the dot and removes duplicates', () => {
    expect(normalizeHubExtensions([' .PDF ', 'pdf', 'Docx'])).toEqual(['pdf', 'docx']);
  });

  it('drops entries that are not plain suffixes', () => {
    expect(normalizeHubExtensions(['*.pdf', 'a/b', 'x,y', '', 'tar.gz', 'odt'])).toEqual(['odt']);
  });

  it('falls back to the default extensions when nothing usable is left', () => {
    expect(normalizeHubExtensions(['  ', '*'])).toEqual(DEFAULT_DOCUMENT_HUB_SETTINGS.extensions);
  });
});

describe('documentHubQuery', () => {
  it('searches multiple base folders recursively for files with any of the extensions', () => {
    const work = { providerId: 'local', uri: 'file:///Volumes/Work' };
    const query = documentHubQuery(
      { ...DEFAULT_DOCUMENT_HUB_SETTINGS, extensions: ['pdf', 'docx'] },
      [HOME, work],
    );

    expect(query.scope).toEqual({
      locations: [HOME, work],
      recurse: true,
      showHidden: false,
      excludedFileNames: DEFAULT_DOCUMENT_HUB_SETTINGS.excludedFileNames,
      excludedDirectoryNames: DEFAULT_DOCUMENT_HUB_SETTINGS.excludedDirectoryNames,
    });
    expect(query.entryKinds).toEqual(['file']);
    expect(query.name).toEqual({ pattern: '*.pdf,*.docx', mode: 'glob', caseSensitive: false });
    expect(query.content).toBeUndefined();
  });

  it('allows hidden paths only when enabled in the hub settings', () => {
    expect(
      documentHubQuery({ ...DEFAULT_DOCUMENT_HUB_SETTINGS, showHidden: true }, [HOME]).scope
        .showHidden,
    ).toBe(true);
  });

  it('uses the normalised extensions, so a typo in settings cannot widen the search', () => {
    const query = documentHubQuery(
      { ...DEFAULT_DOCUMENT_HUB_SETTINGS, extensions: ['.PDF', '*'] },
      [HOME],
    );

    expect(query.name?.pattern).toBe('*.pdf');
  });
});
