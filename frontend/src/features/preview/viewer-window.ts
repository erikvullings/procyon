import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary } from '../../models';

/** Query parameter that makes the frontend mount only the F3 viewer for one file. */
export const VIEWER_QUERY_PARAM = 'viewer';

/** Everything a separate viewer window needs to open the same view an in-pane viewer would. */
export interface ViewerWindowRequest {
  readonly entry: EntrySummary;
  readonly workspaceId?: string;
  readonly initialSearch?: {
    readonly query: string;
    readonly regex: boolean;
    readonly caseSensitive: boolean;
    readonly wholeWord: boolean;
  };
  readonly initialPage?: number;
}

function toBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
}

function fromBase64Url(encoded: string): string {
  const padded = encoded.replaceAll('-', '+').replaceAll('_', '/');
  const binary = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, '='));
  return new TextDecoder().decode(Uint8Array.from(binary, (character) => character.charCodeAt(0)));
}

/** The entry fields the viewer reads; bulky or transient ones (content-search matches, Git
 * status, ...) are left out so the token stays small enough for the window URL. */
function slimEntry(entry: EntrySummary): EntrySummary {
  const { id, location, name, kind, hidden, readOnly, metadataRevision } = entry;
  return {
    id,
    location,
    name,
    kind,
    hidden,
    readOnly,
    metadataRevision,
    ...(entry.extension === undefined ? {} : { extension: entry.extension }),
    ...(entry.size === undefined ? {} : { size: entry.size }),
    ...(entry.modifiedAt === undefined ? {} : { modifiedAt: entry.modifiedAt }),
    ...(entry.mimeType === undefined ? {} : { mimeType: entry.mimeType }),
  };
}

/** Encodes a request as a URL-safe token (base64url of JSON). */
export function encodeViewerRequest(request: ViewerWindowRequest): string {
  return toBase64Url(JSON.stringify({ ...request, entry: slimEntry(request.entry) }));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isViewerEntry(value: unknown): value is EntrySummary {
  if (!isRecord(value) || !isRecord(value.location)) return false;
  return (
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    value.kind === 'file' &&
    typeof value.location.providerId === 'string' &&
    typeof value.location.uri === 'string'
  );
}

/** Parses `?viewer=<token>`; returns `undefined` for a missing or malformed token. */
export function decodeViewerRequest(search: string): ViewerWindowRequest | undefined {
  const token = new URLSearchParams(search).get(VIEWER_QUERY_PARAM);
  if (token === null || token === '') return undefined;
  try {
    const parsed: unknown = JSON.parse(fromBase64Url(token));
    if (!isRecord(parsed) || !isViewerEntry(parsed.entry)) return undefined;
    return parsed as unknown as ViewerWindowRequest;
  } catch {
    return undefined;
  }
}

/**
 * Opens the viewer for `request` in its own window: an OS window on desktop, a popup in the
 * browser. Resolves `false` when no window could be opened (e.g. a blocked popup) so the caller
 * can fall back to the in-pane viewer.
 */
export async function openViewerWindow(
  client: Pick<FileManagerClient, 'openViewerWindow'>,
  request: ViewerWindowRequest,
): Promise<boolean> {
  const token = encodeViewerRequest(request);
  if (client.openViewerWindow !== undefined) {
    try {
      await client.openViewerWindow(token, request.entry.name);
      return true;
    } catch {
      return false;
    }
  }
  const url = new URL(globalThis.location.href);
  url.search = `?${VIEWER_QUERY_PARAM}=${token}`;
  url.hash = '';
  const opened = globalThis.open(url.toString(), '_blank', 'popup=yes,width=1000,height=720');
  return opened !== null;
}
