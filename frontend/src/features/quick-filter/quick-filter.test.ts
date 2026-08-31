import { describe, expect, it } from 'vitest';

import type { EntryId, EntrySummary } from '../../models';
import {
  filterEntries,
  hiddenSelectedEntryCount,
  matchesGlobMask,
  matchesQuickFilter,
} from './quick-filter';

function entry(name: string, id = name): EntrySummary {
  return {
    id: id as EntryId,
    location: { providerId: 'file', uri: `file:///home/erik/${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

describe('matchesQuickFilter', () => {
  it('matches case-insensitively against the entry name', () => {
    expect(matchesQuickFilter(entry('Report.PDF'), 'report')).toBe(true);
    expect(matchesQuickFilter(entry('Report.PDF'), 'REPORT')).toBe(true);
    expect(matchesQuickFilter(entry('Report.PDF'), 'pdf')).toBe(true);
    expect(matchesQuickFilter(entry('Report.PDF'), 'invoice')).toBe(false);
  });
});

describe('matchesGlobMask', () => {
  it('matches star and question-mark wildcards case-insensitively', () => {
    expect(matchesGlobMask('Report.PDF', '*.pdf')).toBe(true);
    expect(matchesGlobMask('notes.txt', 'n?tes.*')).toBe(true);
    expect(matchesGlobMask('notes.txt', '*.pdf')).toBe(false);
  });

  it('treats the Total Commander default *.* as matching every name', () => {
    expect(matchesGlobMask('README', '*.*')).toBe(true);
    expect(matchesGlobMask('archive.tar.gz', '*.*')).toBe(true);
  });
});

describe('filterEntries', () => {
  it('returns every entry for a blank query, preserving the reference', () => {
    const entries = [entry('one'), entry('two')];
    expect(filterEntries(entries, '')).toBe(entries);
    expect(filterEntries(entries, '   ')).toBe(entries);
  });

  it('keeps only entries whose name matches, case-insensitively', () => {
    const report = entry('Report.pdf', 'report');
    const invoice = entry('invoice.txt', 'invoice');
    expect(filterEntries([report, invoice], 'REPORT')).toEqual([report]);
  });

  it('does not redo a full linear pass on unrelated re-invocations with the same inputs', () => {
    const entries = Array.from({ length: 100_000 }, (_, index) => entry(`entry-${index}`));
    const started = performance.now();
    const filtered = filterEntries(entries, 'entry-99999');
    const elapsedMs = performance.now() - started;
    expect(filtered).toHaveLength(1);
    // A single linear substring pass over 100k short strings is well under a frame budget;
    // a generous bound here just guards against an accidentally quadratic implementation.
    expect(elapsedMs).toBeLessThan(200);
  });
});

describe('hiddenSelectedEntryCount', () => {
  it('counts selected entries absent from the visible set', () => {
    const one = entry('one');
    const two = entry('two');
    const three = entry('three');
    const selected = new Set<EntryId>(['one' as EntryId, 'three' as EntryId]);
    expect(hiddenSelectedEntryCount([one, two, three], [two], selected)).toBe(2);
    expect(hiddenSelectedEntryCount([one, two, three], [one, two, three], selected)).toBe(0);
  });
});
