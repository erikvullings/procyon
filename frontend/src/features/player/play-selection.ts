/**
 * Turns a pane selection (files and folders) into queue items for the audio player (task 0263).
 * Selected files keep the pane's displayed order; folders are listed with the pane's own sort and
 * hidden/folders-first settings so the queue matches what the user sees. Entries come from the
 * pane's real locations, so search results, the collection basket and other virtual folders work
 * the same as ordinary directories.
 */
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import type { ListDirectoryRequest } from '../../models/requests';
import { AUDIO_EXTENSIONS, VIDEO_EXTENSIONS } from '../directory-table/entry-icons';
import { isParentEntry } from '../panes/parent-entry';
import type { QueueItem } from './player-state';
import { createQueueItem } from './queue-items';

/** Most tracks one action may queue, so a recursive pick of a huge tree stays bounded. */
export const MAX_QUEUED_TRACKS = 5000;
const MAX_FOLDER_DEPTH = 8;

export type QueueMode = 'play' | 'enqueue' | 'next';

export interface PlaySelectionOptions {
  readonly client: Pick<FileManagerClient, 'listDirectory'>;
  readonly workspaceId: string;
  /** The pane tab's view settings (sort, hidden files, folders first). */
  readonly view: Pick<ListDirectoryRequest, 'sort' | 'showHidden' | 'foldersFirst'>;
  readonly recursive: boolean;
  readonly signal?: AbortSignal;
}

export interface CollectedMedia {
  readonly entries: readonly EntrySummary[];
  /** True when {@link MAX_QUEUED_TRACKS} stopped the collection early. */
  readonly truncated: boolean;
}

const MEDIA_EXTENSIONS: ReadonlySet<string> = new Set([...AUDIO_EXTENSIONS, ...VIDEO_EXTENSIONS]);

export function isPlayableMedia(entry: EntrySummary): boolean {
  const extension =
    entry.extension?.toLowerCase() ?? entry.name.split('.').slice(1).pop()?.toLowerCase();
  return entry.kind === 'file' && extension !== undefined && MEDIA_EXTENSIONS.has(extension);
}

/** True when at least one selected entry is playable media or a folder that may contain some. */
export function hasPlayableSelection(entries: readonly EntrySummary[]): boolean {
  return entries.some(
    (entry) => !isParentEntry(entry.id) && (entry.kind === 'directory' || isPlayableMedia(entry)),
  );
}

async function listAll(options: PlaySelectionOptions, entry: EntrySummary) {
  const paneId = crypto.randomUUID();
  const listed: EntrySummary[] = [];
  let continuationToken: string | undefined;
  do {
    const snapshot = await options.client.listDirectory(
      {
        workspaceId: options.workspaceId,
        // A throwaway pane id, so the listing never disturbs a real pane's own request.
        paneId,
        requestId: crypto.randomUUID(),
        location: entry.location,
        ...(continuationToken === undefined ? {} : { continuationToken }),
        ...options.view,
      },
      options.signal,
    );
    listed.push(...snapshot.entries);
    continuationToken = snapshot.hasMore ? snapshot.continuationToken : undefined;
  } while (continuationToken !== undefined && listed.length < MAX_QUEUED_TRACKS * 4);
  return listed;
}

/** Collects playable media from `entries`, in order, descending into folders as configured. */
export async function collectMedia(
  entries: readonly EntrySummary[],
  options: PlaySelectionOptions,
): Promise<CollectedMedia> {
  const collected: EntrySummary[] = [];
  const seen = new Set<string>();
  let truncated = false;

  async function visit(entry: EntrySummary, depth: number): Promise<void> {
    if (truncated || options.signal?.aborted === true || isParentEntry(entry.id)) return;
    if (entry.kind === 'file') {
      if (!isPlayableMedia(entry) || seen.has(entry.location.uri)) return;
      if (collected.length >= MAX_QUEUED_TRACKS) {
        truncated = true;
        return;
      }
      seen.add(entry.location.uri);
      collected.push(entry);
      return;
    }
    // Symlinked folders are never followed (spec §35); only real directories are entered.
    if (entry.kind !== 'directory') return;
    const children = await listAll(options, entry);
    for (const child of children) {
      if (child.kind === 'directory') {
        if (options.recursive && depth < MAX_FOLDER_DEPTH) await visit(child, depth + 1);
      } else {
        await visit(child, depth + 1);
      }
    }
  }

  for (const entry of entries) await visit(entry, 0);
  return { entries: collected, truncated };
}

export function queueTitle(entry: EntrySummary): string {
  const dot = entry.name.lastIndexOf('.');
  return dot > 0 ? entry.name.slice(0, dot) : entry.name;
}

export function queueItemsFromEntries(entries: readonly EntrySummary[]): QueueItem[] {
  return entries.map((entry) => createQueueItem(entry.location, queueTitle(entry), undefined));
}
