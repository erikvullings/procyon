import m from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';
import { isParentEntry } from '../panes/parent-entry';
import { hasSpecificEntryIcon } from './entry-icons';

type IconClient = Pick<FileManagerClient, 'getFileIcon'>;

function cacheKey(entry: EntrySummary): string | undefined {
  if (isParentEntry(entry.id)) return undefined;
  if (entry.kind === 'directory') return 'directory';
  return `file:${(entry.extension ?? '').toLocaleLowerCase()}`;
}

function pngDataUri(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:image/png;base64,${btoa(binary)}`;
}

/** Lazily resolves and caches native icons without delaying directory row rendering. */
export class NativeIconLoader {
  private readonly icons = new Map<string, string | undefined>();
  private readonly pending = new Set<string>();

  constructor(
    private readonly client: IconClient,
    private readonly redraw: () => void = m.redraw,
  ) {}

  iconDataUri(entry: EntrySummary): string | undefined {
    if (hasSpecificEntryIcon(entry)) return undefined;
    const key = cacheKey(entry);
    if (key === undefined) return undefined;
    if (this.icons.has(key)) return this.icons.get(key);
    if (this.pending.has(key)) return undefined;

    this.pending.add(key);
    void this.client
      .getFileIcon(entry.location.uri)
      .then((bytes) => this.icons.set(key, bytes === undefined ? undefined : pngDataUri(bytes)))
      .catch(() => this.icons.set(key, undefined))
      .finally(() => {
        this.pending.delete(key);
        this.redraw();
      });
    return undefined;
  }
}
