/**
 * Serialises tracks into M3U8, PLS or XSPF text (task 0264). Paths are written relative to the
 * playlist's directory when every track shares the playlist's provider root, otherwise absolute;
 * the result reports which style was actually used so the caller can warn about a fallback.
 */
import type { Location } from '../../models/location';
import type { PlaylistFormat } from './playlist-parser';

export type PlaylistPathStyle = 'relative' | 'absolute';

/** The formats an export can produce (legacy `.m3u` is read-only, it has no defined encoding). */
export type PlaylistExportFormat = Extract<PlaylistFormat, 'm3u8' | 'pls' | 'xspf'>;

export const PLAYLIST_EXPORT_FORMATS: readonly PlaylistExportFormat[] = ['m3u8', 'pls', 'xspf'];

export interface PlaylistExportTrack {
  readonly location: Location;
  readonly title: string | undefined;
  readonly durationSeconds: number | undefined;
}

export interface WrittenPlaylist {
  readonly content: string;
  /** The style actually used: `absolute` when relative paths were requested but impossible. */
  readonly pathStyle: PlaylistPathStyle;
  readonly fellBackToAbsolute: boolean;
}

interface UriParts {
  /** Scheme, authority and - for archive URIs - the part up to and including `!`. */
  readonly root: string;
  readonly segments: readonly string[];
}

function splitUri(uri: string): UriParts | undefined {
  const match = /^([a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^/]*)(\/.*)?$/.exec(uri);
  if (match === null) return undefined;
  const origin = match[1] ?? '';
  const path = (match[2] ?? '').split(/[?#]/)[0] ?? '';
  const bang = path.indexOf('!/');
  const root = bang === -1 ? origin : origin + path.slice(0, bang + 1);
  const inner = bang === -1 ? path : path.slice(bang + 1);
  return { root, segments: inner.split('/').filter((segment) => segment.length > 0) };
}

function decode(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/** The relative path (decoded, `/` separated) from `directory` to `target`, if both share a root. */
function relativePath(directory: Location, target: Location): string | undefined {
  const from = splitUri(directory.uri);
  const to = splitUri(target.uri);
  if (from === undefined || to === undefined || from.root !== to.root) return undefined;
  if (directory.providerId !== target.providerId) return undefined;
  let common = 0;
  while (
    common < from.segments.length &&
    common < to.segments.length - 1 &&
    from.segments[common] === to.segments[common]
  ) {
    common += 1;
  }
  const up = from.segments.slice(common).map(() => '..');
  return [...up, ...to.segments.slice(common)].map(decode).join('/');
}

/** The location as an absolute path: a native-style path for local files, else the full URI. */
function absolutePath(location: Location): string {
  const parts = splitUri(location.uri);
  if (parts === undefined) return location.uri;
  if (location.uri.startsWith('file://')) {
    const path = parts.segments.map(decode).join('/');
    return /^[a-zA-Z]:$/.test(parts.segments[0] ?? '') ? path : `/${path}`;
  }
  return location.uri;
}

function singleLine(text: string): string {
  return text.replace(/[\r\n]+/g, ' ').trim();
}

function escapeXml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function encodeUriPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

function fileNameTitle(location: Location): string {
  const name = decode(location.uri.split(/[?#]/)[0]?.split('/').pop() ?? location.uri);
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}

export function writePlaylist(
  format: PlaylistExportFormat,
  tracks: readonly PlaylistExportTrack[],
  playlistDirectory: Location,
  requestedStyle: PlaylistPathStyle,
): WrittenPlaylist {
  const relativePaths =
    requestedStyle === 'relative'
      ? tracks.map((track) => relativePath(playlistDirectory, track.location))
      : [];
  const useRelative =
    requestedStyle === 'relative' && relativePaths.every((path) => path !== undefined);
  const pathStyle: PlaylistPathStyle = useRelative ? 'relative' : 'absolute';
  const targets = tracks.map((track, index) => {
    const relative = relativePaths[index];
    return useRelative && relative !== undefined ? relative : absolutePath(track.location);
  });
  const titles = tracks.map((track) => singleLine(track.title ?? fileNameTitle(track.location)));
  const seconds = tracks.map((track) =>
    track.durationSeconds === undefined ? -1 : Math.round(track.durationSeconds),
  );

  let content: string;
  if (format === 'm3u8') {
    const lines = ['#EXTM3U'];
    tracks.forEach((_, index) => {
      lines.push(`#EXTINF:${seconds[index]},${titles[index]}`, singleLine(targets[index] ?? ''));
    });
    content = `${lines.join('\n')}\n`;
  } else if (format === 'pls') {
    const lines = ['[playlist]'];
    tracks.forEach((_, index) => {
      const number = index + 1;
      lines.push(
        `File${number}=${singleLine(targets[index] ?? '')}`,
        `Title${number}=${titles[index]}`,
        `Length${number}=${seconds[index]}`,
      );
    });
    lines.push(`NumberOfEntries=${tracks.length}`, 'Version=2');
    content = `${lines.join('\n')}\n`;
  } else {
    const entries = tracks.map((track, index) => {
      const target = targets[index] ?? '';
      // XSPF locations are URIs: relative paths are re-encoded, absolute ones are the entry's URI.
      const location = useRelative ? encodeUriPath(target) : track.location.uri;
      const duration =
        track.durationSeconds === undefined
          ? ''
          : `<duration>${Math.round(track.durationSeconds * 1000)}</duration>`;
      return `    <track><location>${escapeXml(location)}</location><title>${escapeXml(titles[index] ?? '')}</title>${duration}</track>`;
    });
    content = `<?xml version="1.0" encoding="UTF-8"?>\n<playlist version="1" xmlns="http://xspf.org/ns/0/">\n  <trackList>\n${entries.join('\n')}\n  </trackList>\n</playlist>\n`;
  }
  return { content, pathStyle, fellBackToAbsolute: requestedStyle === 'relative' && !useRelative };
}
