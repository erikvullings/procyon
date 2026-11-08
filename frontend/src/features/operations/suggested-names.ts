import type { EntrySummary, Location } from '../../models';
import { isParentEntry } from '../panes/parent-entry';

/** A file's name without its last extension; folders and dotfiles keep their whole name. */
export function nameWithoutExtension(entry: Pick<EntrySummary, 'name' | 'kind'>): string {
  if (entry.kind === 'directory') return entry.name;
  const dot = entry.name.lastIndexOf('.');
  return dot > 0 ? entry.name.slice(0, dot) : entry.name;
}

/** The F7 default: the entry under the cursor (a file without its extension). */
export function suggestedFolderName(cursor: EntrySummary | undefined): string {
  return cursor === undefined || isParentEntry(cursor.id) ? '' : nameWithoutExtension(cursor);
}

function lastSegment(location: Location): string | undefined {
  const raw = location.uri
    .replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/iu, '')
    .replace(/[/!]+$/u, '')
    .split(/[/!]/u)
    .at(-1);
  if (raw === undefined || raw === '') return undefined;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

/**
 * The archive default: the one packed item's name, else the name of the folder being packed from.
 * Falls back to "archive" when neither is known (e.g. at a filesystem root).
 */
export function suggestedArchiveName(
  sources: readonly Location[],
  entries: readonly EntrySummary[],
  directory: Location | undefined,
): string {
  const only = sources.length === 1 ? sources[0] : undefined;
  if (only !== undefined) {
    const entry = entries.find((candidate) => candidate.location.uri === only.uri);
    const name = entry === undefined ? lastSegment(only) : nameWithoutExtension(entry);
    if (name !== undefined && name !== '') return name;
  }
  return (directory === undefined ? undefined : lastSegment(directory)) ?? 'archive';
}
