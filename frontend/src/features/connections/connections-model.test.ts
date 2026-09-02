import { describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Connection, ConnectionConfiguration } from '../../models';
import {
  connectionForLocation,
  connectionStatusGlyph,
  connectionStatusLabel,
  isBrowsable,
  remoteRootLocation,
  saveConnection,
  sftpRootLocation,
  sftpStartPathForConnection,
  upsertConnection,
  validateConnectionDraft,
  withoutConnection,
} from './connections-model';

function sshConfiguration(
  overrides: Partial<Extract<ConnectionConfiguration, { kind: 'ssh' }>> = {},
): ConnectionConfiguration {
  return {
    kind: 'ssh',
    host: 'example.test',
    port: 22,
    username: 'erik',
    startPath: null,
    authentication: 'password',
    hostKeyPolicy: 'promptOnFirstUse',
    keepaliveSeconds: null,
    ...overrides,
  };
}

function sampleConnection(overrides: Partial<Connection> = {}): Connection {
  return {
    id: 'connection-1',
    name: 'Home Server',
    kind: 'ssh',
    configuration: sshConfiguration(),
    hasCredential: true,
    status: 'disconnected',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
    ...overrides,
  };
}

describe('connectionStatusGlyph', () => {
  it('shows a filled dot only when connected', () => {
    expect(connectionStatusGlyph('connected')).toBe('●');
    expect(connectionStatusGlyph('disconnected')).toBe('○');
    expect(connectionStatusGlyph('connecting')).toBe('○');
    expect(connectionStatusGlyph('reconnecting')).toBe('○');
    expect(connectionStatusGlyph('authenticationRequired')).toBe('○');
    expect(connectionStatusGlyph('failed')).toBe('○');
  });
});

describe('connectionStatusLabel', () => {
  it('has a distinct human-readable label for every status', () => {
    const statuses: Connection['status'][] = [
      'disconnected',
      'connecting',
      'connected',
      'reconnecting',
      'authenticationRequired',
      'hostKeyUnverified',
      'hostKeyMismatch',
      'failed',
    ];
    const labels = statuses.map(connectionStatusLabel);
    expect(new Set(labels).size).toBe(statuses.length);
    for (const label of labels) expect(label.length).toBeGreaterThan(0);
  });

  it('distinguishes an unverified host key from a changed one', () => {
    expect(connectionStatusLabel('hostKeyUnverified')).not.toBe(
      connectionStatusLabel('hostKeyMismatch'),
    );
  });
});

describe('isBrowsable', () => {
  it('is true for an ssh connection', () => {
    expect(isBrowsable(sampleConnection({ kind: 'ssh' }))).toBe(true);
  });

  it('is true for FTP and FTPS connections', () => {
    for (const kind of ['ftp', 'ftps'] as const) {
      expect(isBrowsable(sampleConnection({ kind }))).toBe(true);
    }
  });

  it('is true for a WebDAV connection', () => {
    expect(isBrowsable(sampleConnection({ kind: 'webDav' }))).toBe(true);
  });

  it('is true for an authorized OneDrive connection with a backend-provided root', () => {
    expect(
      isBrowsable(
        sampleConnection({
          kind: 'oneDrive',
          configuration: { kind: 'oneDrive', accountHint: null },
          hasCredential: true,
          rootLocation: 'onedrive://11111111-1111-4111-8111-111111111111/',
        }),
      ),
    ).toBe(true);
  });

  it('is false for a OneDrive connection that still needs authorization', () => {
    expect(
      isBrowsable(
        sampleConnection({
          kind: 'oneDrive',
          configuration: { kind: 'oneDrive', accountHint: null },
          hasCredential: false,
          rootLocation: 'onedrive://11111111-1111-4111-8111-111111111111/',
        }),
      ),
    ).toBe(false);
  });

  it('is true for an S3 connection', () => {
    expect(isBrowsable(sampleConnection({ kind: 's3' }))).toBe(true);
  });

  it('is false for kinds without a provider', () => {
    expect(isBrowsable(sampleConnection({ kind: 'smb' }))).toBe(false);
  });
});

describe('remoteRootLocation for OneDrive', () => {
  it('uses the canonical backend-provided virtual root', () => {
    expect(
      remoteRootLocation(
        sampleConnection({
          id: '11111111-1111-4111-8111-111111111111',
          kind: 'oneDrive',
          configuration: { kind: 'oneDrive', accountHint: null },
          hasCredential: true,
          rootLocation: 'onedrive://11111111-1111-4111-8111-111111111111/',
        }),
      ),
    ).toEqual({
      providerId: 'onedrive',
      uri: 'onedrive://11111111-1111-4111-8111-111111111111/',
    });
  });
});

describe('remoteRootLocation for WebDAV', () => {
  function webDavConnection(pathPrefix: string | null): Connection {
    return sampleConnection({
      id: '11111111-1111-4111-8111-111111111111',
      kind: 'webDav',
      configuration: {
        kind: 'webDav',
        baseUrl: 'https://cloud.example.test/dav',
        username: 'erik',
        authentication: 'basic',
        pathPrefix,
      },
    });
  }

  it('builds a webdav:// root location at / when no path prefix is configured', () => {
    const location = remoteRootLocation(webDavConnection(null));
    expect(location.providerId).toBe('webdav');
    expect(location.uri).toBe('webdav://11111111-1111-4111-8111-111111111111/');
  });

  it('builds a webdav:// location under the configured path prefix', () => {
    const location = remoteRootLocation(webDavConnection('/Photos'));
    expect(location.uri).toBe('webdav://11111111-1111-4111-8111-111111111111/Photos');
  });
});

describe('remoteRootLocation for S3', () => {
  it('opens the configured key prefix', () => {
    const connection = sampleConnection({
      id: '11111111-1111-4111-8111-111111111111',
      kind: 's3',
      configuration: {
        kind: 's3',
        accessKeyId: 'AKIAEXAMPLE',
        bucket: 'documents',
        startPath: 'archive/2026',
      },
    });

    expect(remoteRootLocation(connection)).toEqual({
      providerId: 's3',
      uri: 's3://11111111-1111-4111-8111-111111111111/archive/2026',
    });
  });
});

describe('connectionForLocation', () => {
  it.each([
    ['ssh', 'sftp://11111111-1111-4111-8111-111111111111/home/erik'],
    ['webDav', 'webdav://11111111-1111-4111-8111-111111111111/team/docs'],
    ['s3', 's3://11111111-1111-4111-8111-111111111111/archive/2026'],
    ['oneDrive', 'onedrive://11111111-1111-4111-8111-111111111111/Documents'],
  ] as const)('resolves a %s URI to its saved connection', (kind, uri) => {
    const connection = sampleConnection({
      id: '11111111-1111-4111-8111-111111111111',
      kind,
      configuration:
        kind === 'ssh'
          ? sshConfiguration({ startPath: '/home/erik' })
          : kind === 'webDav'
            ? {
                kind,
                baseUrl: 'https://example.test/dav',
                username: 'erik',
                authentication: 'basic',
                pathPrefix: '/team',
              }
            : kind === 's3'
              ? {
                  kind,
                  accessKeyId: 'AKIAEXAMPLE',
                  bucket: 'documents',
                  startPath: '/archive',
                }
              : { kind, accountHint: null },
    });

    expect(connectionForLocation({ providerId: 'remote', uri }, [connection])).toBe(connection);
  });
});

describe('sftpRootLocation', () => {
  it('builds an sftp:// root location for the connection id', () => {
    const location = sftpRootLocation('11111111-1111-4111-8111-111111111111');
    expect(location.providerId).toBe('sftp');
    expect(location.uri).toBe('sftp://11111111-1111-4111-8111-111111111111/');
  });

  it('builds an sftp:// location for an explicit remote start path', () => {
    const location = sftpRootLocation('11111111-1111-4111-8111-111111111111', '/home/erik');
    expect(location.providerId).toBe('sftp');
    expect(location.uri).toBe('sftp://11111111-1111-4111-8111-111111111111/home/erik');
  });

  it('normalizes a start path missing a leading slash', () => {
    const location = sftpRootLocation('11111111-1111-4111-8111-111111111111', 'home/erik');
    expect(location.uri).toBe('sftp://11111111-1111-4111-8111-111111111111/home/erik');
  });
});

describe('sftpStartPathForConnection', () => {
  it('uses explicit ssh startPath when configured', () => {
    expect(
      sftpStartPathForConnection(
        sampleConnection({ configuration: sshConfiguration({ startPath: '/srv/data' }) }),
      ),
    ).toBe('/srv/data');
  });

  it('falls back to the server root when startPath is not configured', () => {
    expect(
      sftpStartPathForConnection(
        sampleConnection({ configuration: sshConfiguration({ startPath: null }) }),
      ),
    ).toBe('/');
  });

  it('normalizes a configured startPath missing leading slash', () => {
    expect(
      sftpStartPathForConnection(
        sampleConnection({ configuration: sshConfiguration({ startPath: 'var/lib' }) }),
      ),
    ).toBe('/var/lib');
  });
});

describe('upsertConnection', () => {
  it('appends a connection not already present', () => {
    const existing = [sampleConnection({ id: 'a' })];
    const next = upsertConnection(existing, sampleConnection({ id: 'b', name: 'NAS' }));
    expect(next.map((c) => c.id)).toEqual(['a', 'b']);
  });

  it('replaces a connection with a matching id in place, preserving order', () => {
    const existing = [
      sampleConnection({ id: 'a', name: 'A' }),
      sampleConnection({ id: 'b', name: 'B' }),
      sampleConnection({ id: 'c', name: 'C' }),
    ];
    const next = upsertConnection(existing, sampleConnection({ id: 'b', name: 'Renamed B' }));
    expect(next.map((c) => c.name)).toEqual(['A', 'Renamed B', 'C']);
  });

  it('does not mutate the input array', () => {
    const existing = [sampleConnection({ id: 'a' })];
    const frozen = Object.freeze([...existing]);
    expect(() =>
      upsertConnection(frozen, sampleConnection({ id: 'a', name: 'Changed' })),
    ).not.toThrow();
    expect(existing[0]?.name).toBe('Home Server');
  });
});

describe('withoutConnection', () => {
  it('removes only the matching connection', () => {
    const existing = [sampleConnection({ id: 'a' }), sampleConnection({ id: 'b' })];
    expect(withoutConnection(existing, 'a').map((c) => c.id)).toEqual(['b']);
  });

  it('is a no-op when the id is not present', () => {
    const existing = [sampleConnection({ id: 'a' })];
    expect(withoutConnection(existing, 'unknown').map((c) => c.id)).toEqual(['a']);
  });
});

describe('validateConnectionDraft', () => {
  it('accepts a well-formed ssh draft', () => {
    expect(
      validateConnectionDraft({ name: 'Home Server', configuration: sshConfiguration() }),
    ).toEqual([]);
  });

  it('reports an empty name', () => {
    const errors = validateConnectionDraft({ name: '   ', configuration: sshConfiguration() });
    expect(errors).toContainEqual({ field: 'name', message: expect.any(String) });
  });

  it('reports every malformed ssh field at once, not just the first', () => {
    const errors = validateConnectionDraft({
      name: 'Home Server',
      configuration: sshConfiguration({ host: '', username: '', port: 0 }),
    });
    const fields = errors.map((error) => error.field);
    expect(fields).toContain('host');
    expect(fields).toContain('username');
    expect(fields).toContain('port');
  });

  it('rejects an out-of-range port', () => {
    const errors = validateConnectionDraft({
      name: 'Home Server',
      configuration: sshConfiguration({ port: 70_000 }),
    });
    expect(errors.map((error) => error.field)).toContain('port');
  });

  it('rejects a configured ssh start path without a leading slash', () => {
    const errors = validateConnectionDraft({
      name: 'Home Server',
      configuration: sshConfiguration({ startPath: 'relative/path' }),
    });
    expect(errors.map((error) => error.field)).toContain('startPath');
  });

  it('does not require ssh-specific fields for a non-ssh kind', () => {
    const errors = validateConnectionDraft({
      name: 'NAS',
      configuration: { kind: 'smb', server: 'nas.local', share: 'media' },
    });
    expect(errors).toEqual([]);
  });
});

function mockClient(
  overrides: Partial<Pick<FileManagerClient, 'createConnection' | 'updateConnection'>> = {},
): FileManagerClient {
  return {
    createConnection: vi.fn().mockResolvedValue(sampleConnection({ id: 'new-id' })),
    updateConnection: vi.fn().mockResolvedValue(sampleConnection({ id: 'existing-id' })),
    ...overrides,
  } as unknown as FileManagerClient;
}

function validDraft() {
  return {
    name: 'Home Server',
    configuration: sshConfiguration(),
    secret: null,
  };
}

describe('saveConnection', () => {
  it('returns validation errors without calling the client when name is empty', async () => {
    const client = mockClient();
    const result = await saveConnection(client, { ...validDraft(), name: '  ' });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.errors.some((e) => e.field === 'name')).toBe(true);
    expect(client.createConnection).not.toHaveBeenCalled();
    expect(client.updateConnection).not.toHaveBeenCalled();
  });

  it('returns a field error for an invalid port without calling the client', async () => {
    const client = mockClient();
    const result = await saveConnection(client, {
      ...validDraft(),
      configuration: sshConfiguration({ port: 0 }),
    });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.errors.some((e) => e.field === 'port')).toBe(true);
    expect(client.createConnection).not.toHaveBeenCalled();
  });

  it('returns a field error for an out-of-range port', async () => {
    const client = mockClient();
    const result = await saveConnection(client, {
      ...validDraft(),
      configuration: sshConfiguration({ port: 65_536 }),
    });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.errors.some((e) => e.field === 'port')).toBe(true);
  });

  it('returns ok: true with no errors for a valid config', async () => {
    const client = mockClient();
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(true);
  });

  it('calls createConnection when no editingId is provided', async () => {
    const client = mockClient();
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(true);
    expect(client.createConnection).toHaveBeenCalledOnce();
    expect(client.updateConnection).not.toHaveBeenCalled();
  });

  it('calls updateConnection when an editingId is provided', async () => {
    const client = mockClient();
    const result = await saveConnection(client, validDraft(), 'connection-1');
    expect(result.ok).toBe(true);
    expect(client.updateConnection).toHaveBeenCalledWith('connection-1', expect.any(Object));
    expect(client.createConnection).not.toHaveBeenCalled();
  });

  it('returns the connection from the client on success', async () => {
    const created = sampleConnection({ id: 'fresh', name: 'Fresh' });
    const client = mockClient({
      createConnection: vi.fn().mockResolvedValue(created),
    });
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.connection).toEqual(created);
  });

  it('maps a network error to a friendly message', async () => {
    const client = mockClient({
      createConnection: vi.fn().mockRejectedValue(new Error('ECONNREFUSED: connection refused')),
    });
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toMatch(/network/i);
  });

  it('maps an already-exists error to a friendly message', async () => {
    const client = mockClient({
      createConnection: vi.fn().mockRejectedValue(new Error('Connection already exists')),
    });
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toMatch(/already exists/i);
  });

  it('returns a fallback message for an unknown error object', async () => {
    const client = mockClient({
      createConnection: vi.fn().mockRejectedValue('boom'),
    });
    const result = await saveConnection(client, validDraft());
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message.length).toBeGreaterThan(0);
  });
});
