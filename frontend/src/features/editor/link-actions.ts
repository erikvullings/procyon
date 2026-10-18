import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary, Location } from '../../models';
import { resolveTargetLocation } from '../playlist/playlist-resolver';
import type { DetectedLink } from './link-detection';

/** A detected link made actionable. */
export type ResolvedLink =
  | { readonly kind: 'url'; readonly url: string }
  | {
      readonly kind: 'path';
      readonly location: Location;
      /** What the path points at. */
      readonly entry: EntrySummary;
      readonly line: number | undefined;
      readonly column: number | undefined;
    };

/** Everything an editor needs to act on a path or URL (task 0271). */
export interface EditorLinkHandlers {
  /** Location of the file being edited; relative paths resolve against its directory. */
  readonly base: Location;
  resolve(link: DetectedLink): Promise<ResolvedLink | undefined>;
  openUrl(url: string): void;
  openInPane(link: Extract<ResolvedLink, { kind: 'path' }>): void;
  reveal(link: Extract<ResolvedLink, { kind: 'path' }>): void;
  editAt(link: Extract<ResolvedLink, { kind: 'path' }>): void;
}

/** Only web links are ever handed to the system opener. */
export function isSafeExternalUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}

/** Expands a leading `~` against the user's home directory. */
function expandHome(path: string, home: string | undefined): string | undefined {
  if (path !== '~' && !path.startsWith('~/')) return path;
  if (home === undefined) return undefined;
  return `${home.replace(/[\\/]+$/u, '')}${path.slice(1)}`;
}

/**
 * Resolves a detected link to something that can be acted on: a URL is checked for a safe scheme;
 * a path is resolved against `base` (or the home directory for `~`) and must exist.
 */
export async function resolveDetectedLink(
  link: DetectedLink,
  base: Location,
  client: Pick<FileManagerClient, 'resolveLinkTarget' | 'getHomeDirectory'>,
  signal?: AbortSignal,
): Promise<ResolvedLink | undefined> {
  if (link.kind === 'url') {
    return isSafeExternalUrl(link.url) ? { kind: 'url', url: link.url } : undefined;
  }
  const expanded = expandHome(link.path, await client.getHomeDirectory(signal));
  if (expanded === undefined) return undefined;
  const location = resolveTargetLocation(base, expanded);
  if (location === undefined) return undefined;
  try {
    const entry = await client.resolveLinkTarget(location, signal);
    return { kind: 'path', location, entry, line: link.line, column: link.column };
  } catch {
    return undefined;
  }
}

/** Selection range covering one-based `line` of `content`, for jumping to a line. */
export function lineRange(
  content: string,
  line: number,
): { readonly from: number; readonly to: number } | undefined {
  if (line < 1) return undefined;
  let from = 0;
  for (let current = 1; current < line; current += 1) {
    const newline = content.indexOf('\n', from);
    if (newline === -1) return undefined;
    from = newline + 1;
  }
  const newline = content.indexOf('\n', from);
  return { from, to: newline === -1 ? content.length : newline };
}
