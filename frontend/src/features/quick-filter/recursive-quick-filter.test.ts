import { describe, expect, it } from 'vitest';
import type { EntryKind, EntrySummary, Location } from '../../models';
import { collectRecursiveQuickFilterEntries } from './recursive-quick-filter';

function location(uri: string): Location {
  return { providerId: 'file', uri };
}

function entry(name: string, kind: EntryKind, uri: string): EntrySummary {
  return {
    id: uri as EntrySummary['id'],
    location: location(uri),
    name,
    kind,
    hidden: false,
    readOnly: false,
    metadataRevision: 0,
  };
}

/** Fake tree: root/alpha.txt, root/sub1/beta.txt, root/sub1/sub2/gamma.log (sub1, sub2 dirs). */
const ROOT: readonly EntrySummary[] = [
  entry('alpha.txt', 'file', 'root/alpha.txt'),
  entry('sub1', 'directory', 'root/sub1'),
];

function listChildrenForTree(): (
  location: Location,
  signal: AbortSignal,
) => Promise<readonly EntrySummary[]> {
  const tree: Record<string, readonly EntrySummary[]> = {
    'root/sub1': [
      entry('beta.txt', 'file', 'root/sub1/beta.txt'),
      entry('sub2', 'directory', 'root/sub1/sub2'),
    ],
    'root/sub1/sub2': [entry('gamma.log', 'file', 'root/sub1/sub2/gamma.log')],
  };
  return (loc, signal) => Promise.resolve(signal.aborted ? [] : (tree[loc.uri] ?? []));
}

function names(entries: readonly EntrySummary[]): string[] {
  return entries.map((e) => e.name).sort();
}

describe('collectRecursiveQuickFilterEntries', () => {
  it('matches across the subtree with the plain-text (fuzzy) query', async () => {
    const result = await collectRecursiveQuickFilterEntries(ROOT, 'txt', listChildrenForTree());
    expect(names(result)).toEqual(['alpha.txt', 'beta.txt']);
  });

  it('honours glob masks just like the current-folder filter', async () => {
    const result = await collectRecursiveQuickFilterEntries(ROOT, '*.log', listChildrenForTree());
    expect(names(result)).toEqual(['gamma.log']);
  });

  it('collects every entry for a blank query (still bounded)', async () => {
    const result = await collectRecursiveQuickFilterEntries(ROOT, '', listChildrenForTree());
    expect(names(result)).toEqual(['alpha.txt', 'beta.txt', 'gamma.log', 'sub1', 'sub2']);
  });

  it('returns only already-loaded matches when aborted before walking', async () => {
    const controller = new AbortController();
    controller.abort();
    const result = await collectRecursiveQuickFilterEntries(ROOT, 'txt', listChildrenForTree(), {
      signal: controller.signal,
    });
    // alpha.txt is in the initial set; beta.txt needs a walk that never runs.
    expect(names(result)).toEqual(['alpha.txt']);
  });

  it('stops descending once maxDirectories is reached', async () => {
    const result = await collectRecursiveQuickFilterEntries(ROOT, 'txt', listChildrenForTree(), {
      maxDirectories: 1,
    });
    // Lists root/sub1 only; sub2 (and thus nothing deeper) is never visited.
    expect(names(result)).toEqual(['alpha.txt', 'beta.txt']);
  });

  it('skips a directory that cannot be listed but keeps walking', async () => {
    const tree = listChildrenForTree();
    const flaky: (location: Location, signal: AbortSignal) => Promise<readonly EntrySummary[]> = (
      loc,
      signal,
    ) => {
      if (loc.uri === 'root/sub1') return Promise.reject(new Error('permission denied'));
      return tree(loc, signal);
    };
    const result = await collectRecursiveQuickFilterEntries(ROOT, 'txt', flaky);
    expect(names(result)).toEqual(['alpha.txt']); // beta/gamma unreachable via sub1
  });
});
