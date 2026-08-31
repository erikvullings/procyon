import { describe, expect, it } from 'vitest';

import type { SavedSearch, SearchQuery } from '../../models';
import {
  deleteSavedSearch,
  renameSavedSearch,
  saveSearch,
  toggleSavedSearchPin,
  updateSavedSearch,
} from './saved-searches';

const query: SearchQuery = {
  schemaVersion: 1,
  scope: {
    locations: [{ providerId: 'local', uri: 'file:///Documents' }],
    recurse: true,
    showHidden: false,
  },
  name: { pattern: '*.pdf', mode: 'glob', caseSensitive: false },
  entryKinds: ['file'],
  mimeTypes: ['application/pdf'],
  gitStatuses: [],
  tags: [],
  metadata: {},
};

const saved: SavedSearch = {
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Reports',
  pinned: false,
  query,
};

describe('saved searches', () => {
  it('supports save, rename, edit, pin, and delete without mutating prior settings', () => {
    const added = saveSearch([], saved);
    const renamed = renameSavedSearch(added, saved.id, 'Quarterly reports');
    const edited = updateSavedSearch(renamed, saved.id, {
      ...query,
      minSizeBytes: 1024,
    });
    const pinned = toggleSavedSearchPin(edited, saved.id);
    const deleted = deleteSavedSearch(pinned, saved.id);

    expect(added).toEqual([saved]);
    expect(renamed[0]?.name).toBe('Quarterly reports');
    expect(edited[0]?.query.minSizeBytes).toBe(1024);
    expect(pinned[0]?.pinned).toBe(true);
    expect(deleted).toEqual([]);
    expect(saved.name).toBe('Reports');
    expect(saved.pinned).toBe(false);
  });

  it('orders pinned searches first while preserving relative order in each group', () => {
    const second = { ...saved, id: '22222222-2222-4222-8222-222222222222', name: 'Second' };
    const third = {
      ...saved,
      id: '33333333-3333-4333-8333-333333333333',
      name: 'Third',
      pinned: true,
    };

    const result = toggleSavedSearchPin([saved, second, third], second.id);

    expect(result.map(({ name }) => name)).toEqual(['Second', 'Third', 'Reports']);
  });
});
