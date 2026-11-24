import type { EntrySummary } from '../../models';

/** The group a file belongs to in the document hub: its lower-cased extension without the dot,
 * or `''` for files without one (a leading dot, as in `.env`, is a hidden name, not an extension). */
export function extensionGroupKey(entry: EntrySummary): string {
  if (entry.extension !== undefined && entry.extension !== '') {
    return entry.extension.replace(/^\./u, '').toLocaleLowerCase();
  }
  const dot = entry.name.lastIndexOf('.');
  return dot <= 0 ? '' : entry.name.slice(dot + 1).toLocaleLowerCase();
}

export interface ExtensionGroup {
  readonly key: string;
  readonly count: number;
}

/** Counts per extension, alphabetical, with the extension-less files last. */
export function extensionGroups(entries: readonly EntrySummary[]): ExtensionGroup[] {
  const counts = new Map<string, number>();
  for (const entry of entries) {
    const key = extensionGroupKey(entry);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([key, count]) => ({ key, count }))
    .sort((left, right) => {
      if (left.key === '' || right.key === '')
        return Number(left.key === '') - Number(right.key === '');
      return left.key < right.key ? -1 : left.key > right.key ? 1 : 0;
    });
}
