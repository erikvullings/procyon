import m from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary, FinderTags } from '../../models';
import { isParentEntry } from '../panes/parent-entry';

type FinderTagsClient = Pick<FileManagerClient, 'getFinderTags'>;

/** Lazily resolves and caches an entry's Finder tags without delaying directory row rendering
 * (task 0136) - mirrors {@link ThumbnailLoader}'s lazy/dedup/cache shape, keyed per entry uri
 * (tags are unique per file, not shared across an extension like native icons are). */
export class FinderTagsLoader {
  private readonly tags = new Map<string, FinderTags | undefined>();
  private readonly pending = new Set<string>();

  constructor(
    private readonly client: FinderTagsClient,
    private readonly redraw: () => void = m.redraw,
  ) {}

  finderTags(entry: EntrySummary): FinderTags | undefined {
    if (isParentEntry(entry.id)) return undefined;
    const uri = entry.location.uri;
    if (this.tags.has(uri)) return this.tags.get(uri);
    if (this.pending.has(uri)) return undefined;

    this.pending.add(uri);
    void this.client
      .getFinderTags(uri)
      .then((tags) => this.tags.set(uri, tags))
      .catch(() => this.tags.set(uri, undefined))
      .finally(() => {
        this.pending.delete(uri);
        this.redraw();
      });
    return undefined;
  }

  /** Seeds the cache with a freshly persisted value (e.g. right after the user edits an entry's
   * tags), so the badge updates immediately without waiting on a refetch. */
  setCached(uri: string, tags: FinderTags): void {
    this.tags.set(uri, tags);
    this.redraw();
  }
}
