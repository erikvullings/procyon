import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  BeginOneDriveAuthorizationResponse,
  Connection,
  ConnectionConfiguration,
  ConnectionId,
  ConnectionSecretInput,
  ConnectionStatus,
  CreateConnectionRequest,
  HostKeyProbe,
  Location,
  OneDriveAuthorizationAttempt,
} from '../../models';

/** The `fm-domain` provider id every `sftp://` location carries (spec §6.5). */
const SFTP_PROVIDER_ID = 'sftp';
/** The `fm-domain` provider id every `webdav://` location carries (task 0147). */
const WEBDAV_PROVIDER_ID = 'webdav';
/** The `fm-domain` provider id every `s3://` location carries (task 0146). */
const S3_PROVIDER_ID = 's3';

const CONNECTION_SCHEME_BY_KIND: Readonly<Record<Connection['kind'], string | undefined>> = {
  ssh: 'sftp:',
  ftp: 'ftp:',
  ftps: 'ftps:',
  oneDrive: 'onedrive:',
  webDav: 'webdav:',
  s3: 's3:',
  smb: undefined,
};

function normalizedRemotePath(path: string | null | undefined): string {
  const trimmed = path?.trim() || '/';
  return trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
}

/**
 * Whether a connection can currently be browsed as a filesystem in a pane
 * (task 0104, 0106, 0147). SSH, FTP/FTPS and WebDAV are the kinds with a real
 * `FileSystemProvider` so far. OneDrive additionally requires a completed
 * authorization before its virtual root can be opened.
 */
export function isBrowsable(connection: Connection): boolean {
  return (
    connection.kind === 'ssh' ||
    connection.kind === 'ftp' ||
    connection.kind === 'ftps' ||
    connection.kind === 'webDav' ||
    connection.kind === 's3' ||
    (connection.kind === 'oneDrive' &&
      connection.hasCredential &&
      connection.rootLocation !== null &&
      connection.rootLocation !== undefined)
  );
}

/** Builds the initial VFS location for any browsable remote connection. */
export function remoteRootLocation(connection: Connection): Location {
  if (connection.configuration.kind === 'ssh') {
    return sftpRootLocation(connection.id, sftpStartPathForConnection(connection));
  }
  if (connection.configuration.kind === 'ftp' || connection.configuration.kind === 'ftps') {
    return {
      providerId: 'ftp',
      uri: `${connection.configuration.kind}://${connection.id}${normalizedRemotePath(connection.configuration.startPath)}`,
    };
  }
  if (connection.configuration.kind === 'webDav') {
    return {
      providerId: WEBDAV_PROVIDER_ID,
      uri: `webdav://${connection.id}${normalizedRemotePath(connection.configuration.pathPrefix)}`,
    };
  }
  if (connection.configuration.kind === 's3') {
    return {
      providerId: S3_PROVIDER_ID,
      uri: `s3://${connection.id}${normalizedRemotePath(connection.configuration.startPath)}`,
    };
  }
  if (
    connection.configuration.kind === 'oneDrive' &&
    connection.hasCredential &&
    connection.rootLocation !== null &&
    connection.rootLocation !== undefined
  ) {
    return { providerId: 'onedrive', uri: connection.rootLocation };
  }
  throw new Error('connection is not browsable');
}

/** Finds the saved profile whose opaque id is the authority of a remote VFS location. */
export function connectionForLocation(
  location: Location,
  connections: readonly Connection[] | undefined,
): Connection | undefined {
  let url: URL;
  try {
    url = new URL(location.uri);
  } catch {
    return undefined;
  }
  return connections?.find(
    (connection) =>
      connection.id === url.hostname && CONNECTION_SCHEME_BY_KIND[connection.kind] === url.protocol,
  );
}

/** Resolves the effective SFTP start path for a browsable connection. */
export function sftpStartPathForConnection(connection: Connection): string {
  if (connection.configuration.kind !== 'ssh') return '/';
  const configured = connection.configuration.startPath?.trim();
  if (configured !== undefined && configured.length > 0) {
    return configured.startsWith('/') ? configured : `/${configured}`;
  }
  return '/';
}

/**
 * Builds an SFTP location for a connection id (spec §6.5).
 *
 * `startPath` must be an absolute remote path; when omitted, `/` is used.
 */
export function sftpRootLocation(connectionId: ConnectionId, startPath = '/'): Location {
  const normalized =
    startPath.length === 0 ? '/' : startPath.startsWith('/') ? startPath : `/${startPath}`;
  return { providerId: SFTP_PROVIDER_ID, uri: `sftp://${connectionId}${normalized}` };
}

/**
 * Status glyph shown next to a connection's name in the `SERVERS` sidebar
 * group (spec §5.5, §20): a filled dot only while connected, an open dot
 * for every other status (including in-progress/degraded ones, which the
 * status label spells out).
 */
export function connectionStatusGlyph(status: ConnectionStatus): '●' | '○' {
  return status === 'connected' ? '●' : '○';
}

/** Human-readable label for a connection's runtime status. */
export function connectionStatusLabel(status: ConnectionStatus): string {
  switch (status) {
    case 'disconnected':
      return t('connections', 'statusDisconnected');
    case 'connecting':
      return t('connections', 'statusConnecting');
    case 'connected':
      return t('connections', 'statusConnected');
    case 'reconnecting':
      return t('connections', 'statusReconnecting');
    case 'authenticationRequired':
      return t('connections', 'statusAuthenticationRequired');
    case 'hostKeyUnverified':
      return t('connections', 'statusHostKeyUnverified');
    case 'hostKeyMismatch':
      return t('connections', 'statusHostKeyMismatch');
    case 'failed':
      return t('connections', 'statusFailed');
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}

/**
 * Merges a created/updated connection into a list by id, appending it if
 * not already present. Never mutates `connections`.
 */
export function upsertConnection(
  connections: readonly Connection[],
  updated: Connection,
): readonly Connection[] {
  const index = connections.findIndex((connection) => connection.id === updated.id);
  if (index === -1) return [...connections, updated];
  const next = [...connections];
  next[index] = updated;
  return next;
}

/** Removes a connection from a list by id. Never mutates `connections`. */
export function withoutConnection(
  connections: readonly Connection[],
  id: ConnectionId,
): readonly Connection[] {
  return connections.filter((connection) => connection.id !== id);
}

/** The fields a connection editor form lets a user type freely. */
export interface ConnectionDraftFields {
  readonly name: string;
  readonly configuration: ConnectionConfiguration;
}

export interface ConnectionDraftValidationError {
  readonly field: string;
  readonly message: string;
}

/** Draft fields plus the write-only secret, passed to `saveConnection`. */
export interface ConnectionSaveDraft extends ConnectionDraftFields {
  readonly secret: ConnectionSecretInput | null;
}

export type ConnectionSaveResult =
  | { readonly ok: true; readonly connection: Connection }
  | {
      readonly ok: false;
      readonly errors: readonly ConnectionDraftValidationError[];
      readonly message: string;
    };

/** Runtime status for connection lifecycle operations in the UI. */
export interface ConnectionLifecycle {
  readonly status: 'idle' | 'loading' | 'saving' | 'error';
  readonly error?: string;
}

/**
 * Validates the fields the connection editor form lets a user type freely
 * (task 0103). Only SSH-specific fields are checked client-side, matching
 * the honestly-scoped surface this task builds (SSH is the only kind
 * meaningfully usable before task 0104/0106); other kinds fall back to the
 * backend's own structured validation, surfaced as a save error.
 */
export function validateConnectionDraft(
  draft: ConnectionDraftFields,
): readonly ConnectionDraftValidationError[] {
  const errors: ConnectionDraftValidationError[] = [];
  if (draft.name.trim().length === 0) {
    errors.push({ field: 'name', message: t('connections', 'enterName') });
  }
  if (
    draft.configuration.kind === 'ssh' ||
    draft.configuration.kind === 'ftp' ||
    draft.configuration.kind === 'ftps'
  ) {
    const { host, username, port, startPath } = draft.configuration;
    if (host.trim().length === 0) {
      errors.push({ field: 'host', message: t('connections', 'enterHost') });
    }
    if (username.trim().length === 0) {
      errors.push({ field: 'username', message: t('connections', 'enterUsername') });
    }
    if (!Number.isInteger(port) || port <= 0 || port > 65_535) {
      errors.push({ field: 'port', message: t('connections', 'enterPort') });
    }
    if (startPath !== null && startPath !== undefined) {
      const normalized = startPath.trim();
      if (normalized.length > 0 && !normalized.startsWith('/')) {
        errors.push({ field: 'startPath', message: t('connections', 'startFolderAbsolute') });
      }
    }
  }
  return errors;
}

// Wrappers over the shared `FileManagerClient` (spec §12): components must
// depend only on this module for connection CRUD/lifecycle, never call
// `client.*Connection*` directly.

function mapConnectionError(error: unknown): string {
  if (!(error instanceof Error)) return t('connections', 'saveFailed');
  const { message } = error;
  if (/already exists/i.test(message)) return t('connections', 'nameExists');
  if (/not found/i.test(message)) return t('connections', 'notFound');
  if (/network|unreachable|ECONNREFUSED/i.test(message)) return t('connections', 'networkError');
  return message;
}

/**
 * Validates, then creates or updates a connection profile in one step.
 * Returns `{ ok: false }` for validation or server errors — callers do not
 * need their own try/catch patterns.
 */
export async function saveConnection(
  client: FileManagerClient,
  draft: ConnectionSaveDraft,
  editingId?: ConnectionId,
): Promise<ConnectionSaveResult> {
  const errors = validateConnectionDraft(draft);
  if (errors.length > 0) {
    return { ok: false, errors, message: errors[0]?.message ?? 'Validation failed.' };
  }
  const request: CreateConnectionRequest = {
    name: draft.name,
    kind: draft.configuration.kind,
    configuration: draft.configuration,
    secret: draft.secret,
  };
  try {
    const connection =
      editingId !== undefined
        ? await client.updateConnection(editingId, request)
        : await client.createConnection(request);
    return { ok: true, connection };
  } catch (e) {
    return { ok: false, errors: [], message: mapConnectionError(e) };
  }
}

/** Lists every stored connection profile with its current runtime status. */
export function loadConnections(
  client: FileManagerClient,
  signal?: AbortSignal,
): Promise<Connection[]> {
  return client.listConnections(signal);
}

/** Deletes a connection profile and its stored credential, if any. */
export function deleteConnection(client: FileManagerClient, id: ConnectionId): Promise<void> {
  return client.deleteConnection(id);
}

/**
 * Attempts to connect. See the backend `fm_connections::ConnectionService`
 * for the honest scope of this operation before task 0104/0106 register a
 * real protocol dialer.
 */
export function connectConnection(
  client: FileManagerClient,
  id: ConnectionId,
): Promise<Connection> {
  return client.connectConnection(id);
}

/** Marks a connection as disconnected. */
export function disconnectConnection(
  client: FileManagerClient,
  id: ConnectionId,
): Promise<Connection> {
  return client.disconnectConnection(id);
}

/** Checks whether a connection's configuration/credential are currently usable. */
export function testConnection(client: FileManagerClient, id: ConnectionId): Promise<Connection> {
  return client.testConnection(id);
}

/** Starts browser-based Microsoft authorization without exposing OAuth secrets to the UI. */
export function beginOneDriveAuthorization(
  client: FileManagerClient,
  id: ConnectionId,
): Promise<BeginOneDriveAuthorizationResponse> {
  return client.beginOneDriveAuthorization(id);
}

/** Polls one backend-owned OneDrive authorization attempt. */
export function getOneDriveAuthorizationAttempt(
  client: FileManagerClient,
  attemptId: string,
): Promise<OneDriveAuthorizationAttempt> {
  return client.getOneDriveAuthorizationAttempt(attemptId);
}

/** Cancels one backend-owned OneDrive authorization attempt. */
export function cancelOneDriveAuthorization(
  client: FileManagerClient,
  attemptId: string,
): Promise<OneDriveAuthorizationAttempt> {
  return client.cancelOneDriveAuthorization(attemptId);
}

/**
 * Probes an SSH connection's currently presented host key without
 * authenticating (task 0104, spec §6.4). Call this when `connect`/`test`
 * report a `hostKeyUnverified`/`hostKeyMismatch` status, to fetch the
 * fingerprint(s) to show the user before deciding whether to accept it.
 */
export function probeSshHostKey(
  client: FileManagerClient,
  id: ConnectionId,
): Promise<HostKeyProbe> {
  return client.probeSshHostKey(id);
}

/**
 * Accepts (persists) a host-key fingerprint for an SSH connection (task
 * 0104, spec §6.4). Only call this with a fingerprint the user has just
 * explicitly confirmed - never silently, even on a first connection.
 */
export function acceptSshHostKey(
  client: FileManagerClient,
  id: ConnectionId,
  fingerprint: string,
): Promise<void> {
  return client.acceptSshHostKey(id, fingerprint);
}
