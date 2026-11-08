import type { EntrySummary, Location } from '../../models';
import { isParentEntry } from '../panes/parent-entry';
import { resolvePreviewKind } from './content-preview';

/** Where the viewed item sits among the pane's media, in the order the pane displays them. */
export interface GalleryPosition {
  /** Zero-based position of the viewed item. */
  readonly index: number;
  readonly count: number;
  readonly previous?: EntrySummary;
  readonly next?: EntrySummary;
}

/** Images, video and audio take part in the lightbox flip-through. */
export function isGalleryEntry(entry: EntrySummary): boolean {
  if (isParentEntry(entry.id)) return false;
  const kind = resolvePreviewKind(entry);
  return kind === 'image' || kind === 'video' || kind === 'audio';
}

/**
 * Locates `current` among the media of `visibleEntries` (already sorted and filtered like the pane)
 * and returns its neighbours. Undefined when the item is not part of that list.
 */
export function galleryPosition(
  visibleEntries: readonly EntrySummary[],
  current: Location,
): GalleryPosition | undefined {
  const media = visibleEntries.filter(isGalleryEntry);
  const index = media.findIndex(
    (entry) =>
      entry.location.providerId === current.providerId && entry.location.uri === current.uri,
  );
  if (index < 0) return undefined;
  const previous = media[index - 1];
  const next = media[index + 1];
  return {
    index,
    count: media.length,
    ...(previous === undefined ? {} : { previous }),
    ...(next === undefined ? {} : { next }),
  };
}
