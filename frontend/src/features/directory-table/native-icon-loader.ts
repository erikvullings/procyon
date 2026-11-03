import m from 'mithril';
import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import type { EntrySummary } from '../../models';
import { WeightedLru } from '../../utilities/weighted-lru';
import { isParentEntry } from '../panes/parent-entry';
import { hasSpecificEntryIcon } from './entry-icons';

type IconClient = Pick<WorkspaceClient, 'getFileIcon'>;

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
/** Characters of data URI kept across all cached icons (about 8 MB of string data). */
const ICON_CACHE_BUDGET_CHARS = 8_000_000;

export class NativeIconLoader {
  private readonly icons = new WeightedLru<string | undefined>(
    ICON_CACHE_BUDGET_CHARS,
    (uri) => uri?.length ?? 64,
  );
  private readonly pending = new Set<string>();

  constructor(
    private readonly client: IconClient,
    private readonly redraw: () => void = m.redraw,
  ) {}

  /** Cache size for memory diagnostics: entries held and the characters of their data URIs. */
  stats(): { readonly count: number; readonly chars: number } {
    let chars = 0;
    for (const uri of this.icons.values()) chars += uri?.length ?? 0;
    return { count: this.icons.size, chars };
  }

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
