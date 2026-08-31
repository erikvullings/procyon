import type { EntryId, EntrySummary } from '../../models';

const PARENT_ENTRY_PREFIX = 'fm:parent:';

function isRootPath(path: string): boolean {
  const normalized = path.replaceAll('/', '\\').replace(/\\+$/, '');
  return (
    path === '/' || /^\\?[A-Za-z]:$/.test(normalized) || /^\\\\[^\\]+\\[^\\]+$/.test(normalized)
  );
}

/** Identifies the presentation-only parent-directory row. */
export function isParentEntry(entryId: EntryId | undefined): boolean {
  return entryId?.startsWith(PARENT_ENTRY_PREFIX) ?? false;
}

/** Prepends `..` outside filesystem roots without mutating backend entries. */
export function withParentEntry(
  path: string,
  entries: readonly EntrySummary[],
): readonly EntrySummary[] {
  if (isRootPath(path)) {
    return entries;
  }
  return [
    {
      id: `${PARENT_ENTRY_PREFIX}${path}`,
      location: { providerId: entries[0]?.location.providerId ?? 'file', uri: path },
      name: '..',
      kind: 'directory',
      hidden: false,
      readOnly: true,
      metadataRevision: 0,
    },
    ...entries,
  ];
}
