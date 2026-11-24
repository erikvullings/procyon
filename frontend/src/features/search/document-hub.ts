import type { Location, SearchQuery } from '../../models';
import { DEFAULT_DOCUMENT_HUB_SETTINGS, type DocumentHubSettings } from '../../models/settings';

const MAX_EXTENSION_LENGTH = 16;
const PLAIN_EXTENSION = /^[a-z0-9_+-]+$/u;

/** Mirrors the backend's sanitising: lower case, no dot, plain suffixes only, no duplicates,
 * and the defaults when nothing usable remains. */
export function normalizeHubExtensions(raw: readonly string[]): string[] {
  const seen = new Set<string>();
  for (const value of raw) {
    const extension = value.trim().replace(/^\.+/u, '').toLocaleLowerCase();
    if (extension.length <= MAX_EXTENSION_LENGTH && PLAIN_EXTENSION.test(extension)) {
      seen.add(extension);
    }
  }
  return seen.size === 0 ? [...DEFAULT_DOCUMENT_HUB_SETTINGS.extensions] : [...seen];
}

/** The document hub as an ordinary recursive search: one comma-separated filename glob per
 * configured extension, files only, below `root` (the resolved base folder). */
export function documentHubQuery(
  settings: DocumentHubSettings,
  root: Location,
  showHidden: boolean,
): SearchQuery {
  return {
    schemaVersion: 2,
    mode: 'name',
    scope: { locations: [root], recurse: true, showHidden },
    name: {
      pattern: normalizeHubExtensions(settings.extensions)
        .map((extension) => `*.${extension}`)
        .join(','),
      mode: 'glob',
      caseSensitive: false,
    },
    entryKinds: ['file'],
    mimeTypes: [],
    gitStatuses: [],
    tags: [],
    metadata: {},
  };
}
