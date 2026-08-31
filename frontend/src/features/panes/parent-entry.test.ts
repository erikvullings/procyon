import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models';
import { isParentEntry, withParentEntry } from './parent-entry';

const entry: EntrySummary = {
  id: 'file',
  location: { providerId: 'file', uri: 'file:///folder/file.txt' },
  name: 'file.txt',
  kind: 'file',
  hidden: false,
  readOnly: false,
  metadataRevision: 1,
};

describe('parent directory entry', () => {
  it.each(['/', 'C:\\', 'C:/', '/C:/', '\\\\server\\share'])(
    'does not add a parent at root %s',
    (path) => {
      expect(withParentEntry(path, [entry])).toEqual([entry]);
    },
  );

  it('adds a synthetic parent as the first entry outside a root', () => {
    const entries = withParentEntry('/folder', [entry]);

    expect(entries.map(({ name }) => name)).toEqual(['..', 'file.txt']);
    expect(isParentEntry(entries[0]?.id)).toBe(true);
  });
});
