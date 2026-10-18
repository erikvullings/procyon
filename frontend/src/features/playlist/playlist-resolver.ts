/**
 * Resolves playlist entries to VFS locations and checks they exist (task 0261). Relative entries
 * resolve against the playlist's own location, so a playlist inside an archive or on a remote
 * provider resolves through the same provider. Remote (`http(s)` and other streaming) entries
 * are classified but never fetched here.
 */
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location } from '../../models/location';
import type { PlaylistTrack } from './playlist-parser';

export type PlaylistTrackStatus =
  | 'checking'
  | 'found'
  | 'missing'
  /** A network URL; shown as text and never fetched without an explicit opt-in. */
  | 'remote';

export interface ResolvedPlaylistTrack extends PlaylistTrack {
  readonly status: PlaylistTrackStatus;
  /** The VFS location the entry points at (absent for remote and unresolvable entries). */
  readonly location: Location | undefined;
}

const URL_SCHEME = /^([a-zA-Z][a-zA-Z0-9+.-]*):\/\//;
const WINDOWS_DRIVE = /^[a-zA-Z]:[\\/]/;

/** True for entries that point across the network rather than at a VFS entry. */
export function isRemoteTarget(target: string): boolean {
  const scheme = URL_SCHEME.exec(target)?.[1]?.toLowerCase();
  return scheme !== undefined && scheme !== 'file';
}

function encodePath(path: string): string {
  return path
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
}

/**
 * Maps a non-remote playlist target to a location, or `undefined` when it cannot be expressed
 * (for example a Windows drive path in a playlist that lives on an SFTP server).
 */
export function resolveTargetLocation(playlist: Location, target: string): Location | undefined {
  const playlistIsLocal = playlist.uri.startsWith('file://');
  try {
    if (/^file:\/\//i.test(target)) {
      return {
        providerId: playlistIsLocal ? playlist.providerId : 'local',
        uri: new URL(target).href,
      };
    }
    if (WINDOWS_DRIVE.test(target)) {
      if (!playlistIsLocal) return undefined;
      return {
        providerId: playlist.providerId,
        uri: new URL(
          `file:///${target.slice(0, 2)}/${encodePath(target.slice(3).replace(/\\/g, '/'))}`,
        ).href,
      };
    }
    if (target.startsWith('\\\\')) return undefined;
    const path = target.replace(/\\/g, '/');
    const href = new URL(encodePath(path).replace(/^%2F/i, '/'), playlist.uri).href;
    return { providerId: playlist.providerId, uri: href };
  } catch {
    return undefined;
  }
}

/** Initial rows: remote entries are final, the rest await an existence check. */
export function initialPlaylistRows(
  playlist: Location,
  tracks: readonly PlaylistTrack[],
): ResolvedPlaylistTrack[] {
  return tracks.map((track) => {
    if (isRemoteTarget(track.target)) {
      return { ...track, status: 'remote', location: undefined };
    }
    const location = resolveTargetLocation(playlist, track.target);
    return location === undefined
      ? { ...track, status: 'missing', location: undefined }
      : { ...track, status: 'checking', location };
  });
}

const CHECK_CONCURRENCY = 8;
const PROGRESS_BATCH = 25;

/**
 * Checks every `checking` row against the VFS, a few at a time, reporting a fresh copy of the
 * rows every batch so a long playlist fills in progressively. Stops quietly when aborted.
 */
export async function checkPlaylistRows(
  client: Pick<FileManagerClient, 'resolveLinkTarget'>,
  rows: readonly ResolvedPlaylistTrack[],
  signal: AbortSignal,
  onProgress: (rows: readonly ResolvedPlaylistTrack[]) => void,
): Promise<readonly ResolvedPlaylistTrack[]> {
  const current = [...rows];
  const pending = current.flatMap((row, index) => (row.status === 'checking' ? [index] : []));
  let next = 0;
  let sinceReport = 0;
  const worker = async (): Promise<void> => {
    while (!signal.aborted) {
      const position = next++;
      const index = pending[position];
      if (index === undefined) return;
      const row = current[index];
      if (row?.location === undefined) continue;
      let status: PlaylistTrackStatus = 'found';
      try {
        const entry = await client.resolveLinkTarget(row.location, signal);
        if (entry.kind === 'directory') status = 'missing';
      } catch {
        status = 'missing';
      }
      if (signal.aborted) return;
      current[index] = { ...row, status };
      sinceReport += 1;
      if (sinceReport >= PROGRESS_BATCH) {
        sinceReport = 0;
        onProgress([...current]);
      }
    }
  };
  await Promise.all(Array.from({ length: CHECK_CONCURRENCY }, worker));
  return current;
}
