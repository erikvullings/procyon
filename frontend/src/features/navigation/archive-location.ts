import type { EntrySummary } from '../../models/entry';
import type { Location } from '../../models/location';

export const ARCHIVE_SUFFIXES = [
  '.tar.bz2',
  '.tar.gz',
  '.tar.xz',
  '.tbz2',
  '.7z',
  '.cbr',
  '.cbz',
  '.epub',
  '.gz',
  '.rar',
  '.tar',
  '.tbz',
  '.tgz',
  '.txz',
  '.zip',
] as const;

/** True when the file name has a supported archive suffix. */
export function hasArchiveSuffix(name: string): boolean {
  const lowerName = name.toLocaleLowerCase('en-US');
  return ARCHIVE_SUFFIXES.some((suffix) => lowerName.endsWith(suffix));
}

/** Returns the folder-like archive root for a supported archive entry: a local file, or a file
 * inside another archive (`archive://outer.zip!/inner.zip` opens as `archive://outer.zip!/inner.zip!`). */
export function archiveRootForEntry(entry: EntrySummary): Location | undefined {
  if (entry.kind !== 'file' || !hasArchiveSuffix(entry.name)) return undefined;
  const { providerId, uri } = entry.location;
  if (providerId === 'local' && uri.startsWith('file://')) {
    return { providerId: 'archive', uri: `archive://${uri.slice('file://'.length)}!/` };
  }
  if (providerId === 'archive' && uri.startsWith('archive://') && !uri.endsWith('!')) {
    return { providerId: 'archive', uri: `${uri}!` };
  }
  return undefined;
}

/** Builds the location of a known entry inside an archive, given the archive's root (from
 * {@link archiveRootForEntry}) and the entry's path relative to the archive root (e.g. from an
 * OPF manifest `href` or a hardcoded path like `META-INF/container.xml`) - a direct read by a
 * path the caller already knows, without listing the archive first. */
export function archiveEntryLocation(archiveRoot: Location, innerPath: string): Location {
  return { providerId: archiveRoot.providerId, uri: `${archiveRoot.uri}${innerPath}` };
}
