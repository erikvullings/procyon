/**
 * Pure parsers for playlist files (task 0261): M3U/M3U8 (`#EXTINF`), PLS (INI) and XSPF (XML).
 * Parsing never throws: malformed input yields whatever tracks could be recovered plus an
 * `error` describing what went wrong, so the viewer can show a partial list instead of failing.
 */

export type PlaylistFormat = 'm3u' | 'm3u8' | 'pls' | 'xspf';

/** One entry of a playlist as written in the file, before any path resolution. */
export interface PlaylistTrack {
  readonly title: string | undefined;
  readonly durationSeconds: number | undefined;
  /** The raw target: a path (absolute or relative), a `file://` URL or a remote URL. */
  readonly target: string;
}

export interface ParsedPlaylist {
  readonly format: PlaylistFormat;
  readonly tracks: readonly PlaylistTrack[];
  /** True when more than {@link MAX_PLAYLIST_TRACKS} entries were present. */
  readonly truncated: boolean;
  /** Set when the file was malformed; `tracks` then holds what could be recovered. */
  readonly error: string | undefined;
}

/** Entries beyond this are dropped so a hostile or huge file cannot stall the viewer. */
export const MAX_PLAYLIST_TRACKS = 50_000;

/** Largest playlist file read for parsing. */
export const MAX_PLAYLIST_BYTES = 8 * 1024 * 1024;

const FORMATS_BY_EXTENSION: Readonly<Record<string, PlaylistFormat>> = {
  m3u: 'm3u',
  m3u8: 'm3u8',
  pls: 'pls',
  xspf: 'xspf',
};

export const PLAYLIST_EXTENSIONS: readonly string[] = Object.keys(FORMATS_BY_EXTENSION);

/** The playlist format implied by a file extension (without dot), if any. */
export function playlistFormatForExtension(
  extension: string | undefined,
): PlaylistFormat | undefined {
  return extension === undefined ? undefined : FORMATS_BY_EXTENSION[extension.toLowerCase()];
}

/**
 * Decodes playlist bytes: honours UTF-8/UTF-16 BOMs; `.m3u8`, PLS and XSPF are UTF-8, while the
 * legacy `.m3u` falls back to Windows-1252 (Latin-1) when the bytes are not valid UTF-8.
 */
export function decodePlaylistBytes(bytes: Uint8Array, format: PlaylistFormat): string {
  if (bytes[0] === 0xff && bytes[1] === 0xfe) return new TextDecoder('utf-16le').decode(bytes);
  if (bytes[0] === 0xfe && bytes[1] === 0xff) return new TextDecoder('utf-16be').decode(bytes);
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return format === 'm3u'
      ? new TextDecoder('windows-1252').decode(bytes)
      : new TextDecoder('utf-8').decode(bytes);
  }
}

export function parsePlaylist(text: string, format: PlaylistFormat): ParsedPlaylist {
  const body = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
  const parsed =
    format === 'pls' ? parsePls(body) : format === 'xspf' ? parseXspf(body) : parseM3u(body);
  const truncated = parsed.tracks.length > MAX_PLAYLIST_TRACKS;
  return {
    format,
    tracks: truncated ? parsed.tracks.slice(0, MAX_PLAYLIST_TRACKS) : parsed.tracks,
    truncated,
    error: parsed.error,
  };
}

interface RawParse {
  readonly tracks: PlaylistTrack[];
  readonly error: string | undefined;
}

function nonEmpty(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed === undefined || trimmed.length === 0 ? undefined : trimmed;
}

/** Durations of zero or less (`-1` means "unknown") are not durations. */
function positiveSeconds(value: number): number | undefined {
  return Number.isFinite(value) && value > 0 ? Math.round(value) : undefined;
}

function parseM3u(text: string): RawParse {
  const tracks: PlaylistTrack[] = [];
  let pendingTitle: string | undefined;
  let pendingDuration: number | undefined;
  for (const rawLine of text.split(/\r\n|\r|\n/)) {
    const line = rawLine.trim();
    if (line.length === 0) continue;
    if (line.startsWith('#')) {
      if (/^#EXTINF:/i.test(line)) {
        const info = line.slice('#EXTINF:'.length);
        const comma = info.indexOf(',');
        const head = comma === -1 ? info : info.slice(0, comma);
        pendingDuration = positiveSeconds(Number.parseFloat(head));
        pendingTitle = comma === -1 ? undefined : nonEmpty(info.slice(comma + 1));
      }
      continue;
    }
    tracks.push({ title: pendingTitle, durationSeconds: pendingDuration, target: line });
    pendingTitle = undefined;
    pendingDuration = undefined;
  }
  return { tracks, error: undefined };
}

function parsePls(text: string): RawParse {
  const files = new Map<number, string>();
  const titles = new Map<number, string>();
  const lengths = new Map<number, number>();
  let sawSection = false;
  for (const rawLine of text.split(/\r\n|\r|\n/)) {
    const line = rawLine.trim();
    if (/^\[playlist\]$/i.test(line)) {
      sawSection = true;
      continue;
    }
    const match = /^(file|title|length)(\d+)\s*=(.*)$/i.exec(line);
    if (match === null) continue;
    const index = Number.parseInt(match[2] ?? '', 10);
    const value = (match[3] ?? '').trim();
    const key = (match[1] ?? '').toLowerCase();
    if (key === 'file') files.set(index, value);
    else if (key === 'title') titles.set(index, value);
    else lengths.set(index, Number.parseFloat(value));
  }
  const tracks = [...files.entries()]
    .sort(([left], [right]) => left - right)
    .filter(([, target]) => target.length > 0)
    .map(
      ([index, target]): PlaylistTrack => ({
        title: nonEmpty(titles.get(index)),
        durationSeconds: positiveSeconds(lengths.get(index) ?? Number.NaN),
        target,
      }),
    );
  return {
    tracks,
    error: sawSection || tracks.length > 0 ? undefined : 'Missing [playlist] section.',
  };
}

function safeDecode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

function parseXspf(text: string): RawParse {
  const document = new DOMParser().parseFromString(text, 'application/xml');
  if (document.getElementsByTagName('parsererror').length > 0) {
    return { tracks: [], error: 'The XSPF file is not well-formed XML.' };
  }
  const tracks: PlaylistTrack[] = [];
  for (const track of Array.from(document.getElementsByTagNameNS('*', 'track'))) {
    const child = (name: string): string | undefined => {
      for (const element of Array.from(track.children)) {
        if (element.localName === name) return nonEmpty(element.textContent ?? undefined);
      }
      return undefined;
    };
    const rawLocation = child('location');
    if (rawLocation === undefined) continue;
    // XSPF locations are URIs. Full URLs stay as written; relative ones are percent-decoded so
    // every format reaches the resolver as a plain path or a full URL.
    const location = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(rawLocation)
      ? rawLocation
      : safeDecode(rawLocation);
    const milliseconds = Number.parseFloat(child('duration') ?? '');
    tracks.push({
      title: child('title'),
      durationSeconds: positiveSeconds(milliseconds / 1000),
      target: location,
    });
  }
  return { tracks, error: undefined };
}
