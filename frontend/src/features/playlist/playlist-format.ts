import type { ResolvedPlaylistTrack } from './playlist-resolver';

/** `m:ss` or `h:mm:ss`; empty when the duration is unknown. */
export function formatTrackDuration(seconds: number | undefined): string {
  if (seconds === undefined) return '';
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = String(total % 60).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
}

function safeDecode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/** The path as the user would read it: the decoded location URI, or the raw entry for streams. */
export function trackDisplayPath(track: ResolvedPlaylistTrack): string {
  return track.location === undefined ? track.target : safeDecode(track.location.uri);
}

/** The title, or the file name of the target when the playlist gave none. */
export function trackDisplayTitle(track: ResolvedPlaylistTrack): string {
  if (track.title !== undefined) return track.title;
  const withoutQuery = track.target.split(/[?#]/)[0] ?? track.target;
  const name = withoutQuery
    .split(/[\\/]/)
    .filter((part) => part.length > 0)
    .pop();
  return safeDecode(name ?? track.target);
}
