import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import type { EntrySummary } from '../../models';
import { WeightedLru } from '../../utilities/weighted-lru';

type ThumbnailSource = Pick<WorkspaceClient, 'getThumbnail'>;
type Size = 'small' | 'medium' | 'large';

const MAX_CONCURRENT = 4;
/** Characters of data URI kept across all map thumbnails (about 16 MB of string data). */
const MAP_THUMBNAIL_BUDGET_CHARS = 16_000_000;
const results = new WeightedLru<string | null>(
  MAP_THUMBNAIL_BUDGET_CHARS,
  (uri) => uri?.length ?? 64,
);
const waiting: (() => void)[] = [];
let active = 0;

/** Cache size for memory diagnostics: entries held and the characters of their data URIs. */
export function mapThumbnailStats(): { readonly count: number; readonly chars: number } {
  let chars = 0;
  for (const uri of results.values()) chars += uri?.length ?? 0;
  return { count: results.size, chars };
}

function dataUri(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:image/jpeg;base64,${btoa(binary)}`;
}

function release(): void {
  active -= 1;
  waiting.shift()?.();
}

async function load(
  client: ThumbnailSource,
  entry: EntrySummary,
  size: Size,
): Promise<string | null> {
  const key = `${entry.location.uri}|${entry.modifiedAt ?? ''}|${size}`;
  const known = results.get(key);
  if (known !== undefined) return known;
  if (active >= MAX_CONCURRENT) await new Promise<void>((resolve) => waiting.push(resolve));
  active += 1;
  try {
    const bytes = await client.getThumbnail(entry.location.uri, size);
    const uri = bytes === undefined ? null : dataUri(bytes);
    results.set(key, uri);
    return uri;
  } catch {
    return null;
  } finally {
    release();
  }
}

/**
 * An `<img>` that fills in a backend thumbnail (task 0134 pipeline) once it arrives, with bounded
 * concurrency. Markers and popups are plain DOM owned by the map, so they cannot rely on Mithril
 * redraws to show a thumbnail that loads later.
 */
export function thumbnailImage(
  client: ThumbnailSource,
  entry: EntrySummary,
  size: Size,
  className: string,
): HTMLImageElement {
  const image = document.createElement('img');
  image.className = className;
  image.alt = entry.name;
  image.draggable = false;
  void load(client, entry, size).then((uri) => {
    if (uri === null) image.classList.add('is-missing');
    else image.src = uri;
  });
  return image;
}
