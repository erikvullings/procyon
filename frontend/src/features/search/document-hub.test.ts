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
  it('searches the base folder recursively for files with any of the extensions', () => {
    const query = documentHubQuery({ extensions: ['pdf', 'docx'], basePath: '~' }, HOME, false);

    expect(query.scope).toEqual({ locations: [HOME], recurse: true, showHidden: false });
    expect(query.entryKinds).toEqual(['file']);
    expect(query.name).toEqual({ pattern: '*.pdf,*.docx', mode: 'glob', caseSensitive: false });
    expect(query.content).toBeUndefined();
  });

  it('follows the show-hidden setting', () => {
    expect(documentHubQuery(DEFAULT_DOCUMENT_HUB_SETTINGS, HOME, true).scope.showHidden).toBe(true);
  });

  it('uses the normalised extensions, so a typo in settings cannot widen the search', () => {
    const query = documentHubQuery({ extensions: ['.PDF', '*'], basePath: '~' }, HOME, false);

    expect(query.name?.pattern).toBe('*.pdf');
  });
});
