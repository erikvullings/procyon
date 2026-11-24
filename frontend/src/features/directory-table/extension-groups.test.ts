import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models';
import { extensionGroupKey, extensionGroups } from './extension-groups';

function entry(name: string, extension?: string): EntrySummary {
  return {
    id: name,
    location: { providerId: 'local', uri: `file:///docs/${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...(extension === undefined ? {} : { extension }),
  };
}

describe('extensionGroupKey', () => {
  it('uses the lower-cased extension without a dot', () => {
    expect(extensionGroupKey(entry('Report.PDF', 'PDF'))).toBe('pdf');
  });

  it('reads the extension from the name when the listing did not provide one', () => {
    expect(extensionGroupKey(entry('slides.pptx'))).toBe('pptx');
  });

  it('puts files without an extension, and dotfiles, in the empty group', () => {
    expect(extensionGroupKey(entry('Makefile'))).toBe('');
    expect(extensionGroupKey(entry('.env'))).toBe('');
  });
});

describe('extensionGroups', () => {
  it('counts files per extension, alphabetically, case-insensitively', () => {
    const groups = extensionGroups([
      entry('b.pdf', 'pdf'),
      entry('a.DOCX', 'DOCX'),
      entry('c.PDF', 'PDF'),
      entry('d.pdf', 'pdf'),
    ]);

    expect(groups).toEqual([
      { key: 'docx', count: 1 },
      { key: 'pdf', count: 3 },
    ]);
  });

  it('lists files without an extension last', () => {
    const groups = extensionGroups([entry('Makefile'), entry('a.md', 'md'), entry('z.txt', 'txt')]);

    expect(groups.map((group) => group.key)).toEqual(['md', 'txt', '']);
  });

  it('is empty for no entries', () => {
    expect(extensionGroups([])).toEqual([]);
  });
});
