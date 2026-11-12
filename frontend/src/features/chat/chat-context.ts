import type { EntrySummary } from '../../models';
import type { ChatAttachment } from './chat-tab-state';

/** Most entries listed in one attachment; the rest are counted, not sent. */
export const MAX_LISTED_ENTRIES = 200;
/** Most characters of attached context per message; matches the backend limit. */
export const MAX_ATTACHED_CHARS = 32 * 1024;
/** Largest file whose contents can be attached. */
export const MAX_ATTACHED_FILE_BYTES = 16 * 1024;

/** Human-readable size with binary units, matching what the folder view shows. */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

/** One line per entry: name, kind, and size or modification date when known. */
function entryLine(entry: EntrySummary): string {
  const details: string[] = [];
  if (entry.kind === 'directory') {
    details.push('folder');
  } else {
    if (entry.size !== undefined) details.push(formatBytes(entry.size));
  }
  if (entry.modifiedAt !== undefined) details.push(`modified ${entry.modifiedAt.slice(0, 10)}`);
  const name = entry.kind === 'directory' ? `${entry.name}/` : entry.name;
  return details.length === 0 ? `- ${name}` : `- ${name} (${details.join(', ')})`;
}

function describe(header: string, entries: readonly EntrySummary[]): string {
  const listed = entries.slice(0, MAX_LISTED_ENTRIES).map(entryLine);
  const rest = entries.length - listed.length;
  if (rest > 0) listed.push(`- … and ${rest} more`);
  return [header, ...listed].join('\n');
}

/** The folder listing as context: names, kinds, sizes and dates. Never file contents. */
export function folderListingAttachment(
  id: string,
  folderUri: string,
  entries: readonly EntrySummary[],
): ChatAttachment {
  return {
    id,
    label: `Folder listing: ${folderUri}`,
    text: describe(`Folder listing of ${folderUri} (${entries.length} entries):`, entries),
  };
}

/** The selected entries as context, with the same details as the listing. */
export function selectionAttachment(
  id: string,
  folderUri: string,
  entries: readonly EntrySummary[],
): ChatAttachment {
  return {
    id,
    label: `Selection: ${entries.length} in ${folderUri}`,
    text: describe(`Selected entries in ${folderUri} (${entries.length}):`, entries),
  };
}

/** The contents of one file the person selected. Sent only on that explicit request. */
export function fileContentAttachment(
  id: string,
  name: string,
  size: number,
  content: string,
): ChatAttachment {
  return {
    id,
    label: `File: ${name}`,
    text: `Contents of ${name} (${formatBytes(size)}):\n${content}`,
  };
}

/** Characters of context already attached, which the next attachment must fit beside. */
export function attachedChars(attachments: readonly ChatAttachment[]): number {
  return attachments.reduce((total, attachment) => total + attachment.text.length, 0);
}
