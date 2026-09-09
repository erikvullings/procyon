import { describe, expect, it, vi } from 'vitest';
import type { Connection, ResolvedRagCitation } from '../../models';
import { navigateToRagCitation, openCreatedConnection } from './app-dialogs';

function connection(overrides: Partial<Connection> = {}): Connection {
  return {
    id: '11111111-1111-4111-8111-111111111111',
    name: 'Work server',
    kind: 'ssh',
    configuration: {
      kind: 'ssh',
      host: 'example.test',
      port: 22,
      username: 'erik',
      startPath: '/srv/work',
      authentication: 'password',
      hostKeyPolicy: 'promptOnFirstUse',
    },
    hasCredential: true,
    status: 'connected',
    createdAt: '2026-08-13T00:00:00Z',
    updatedAt: '2026-08-13T00:00:00Z',
    ...overrides,
  };
}

function context() {
  return {
    setConnectionsManagerOpen: vi.fn(),
    navigateActiveLocation: vi.fn().mockResolvedValue(undefined),
    redraw: vi.fn(),
  };
}

describe('openCreatedConnection', () => {
  it('closes the manager and opens a newly created connection at its configured root', async () => {
    const ctx = context();

    await openCreatedConnection(connection(), undefined, ctx);

    expect(ctx.setConnectionsManagerOpen).toHaveBeenCalledExactlyOnceWith(false);
    expect(ctx.navigateActiveLocation).toHaveBeenCalledExactlyOnceWith({
      providerId: 'sftp',
      uri: 'sftp://11111111-1111-4111-8111-111111111111/srv/work',
    });
  });

  it('does not navigate after editing an existing connection', async () => {
    const ctx = context();

    await openCreatedConnection(connection(), connection().id, ctx);

    expect(ctx.navigateActiveLocation).not.toHaveBeenCalled();
    expect(ctx.setConnectionsManagerOpen).not.toHaveBeenCalled();
  });

  it('waits for OneDrive authorization before opening the connection', async () => {
    const ctx = context();

    await openCreatedConnection(
      connection({
        kind: 'oneDrive',
        configuration: { kind: 'oneDrive', accountHint: null },
        hasCredential: false,
        status: 'authenticationRequired',
        rootLocation: 'onedrive://11111111-1111-4111-8111-111111111111/',
      }),
      undefined,
      ctx,
    );

    expect(ctx.navigateActiveLocation).not.toHaveBeenCalled();
    expect(ctx.setConnectionsManagerOpen).not.toHaveBeenCalled();
  });
});

describe('navigateToRagCitation', () => {
  it('opens the containing folder and selects the decoded source name', async () => {
    const resolve = vi.fn().mockResolvedValue({
      available: true,
      entryId: '11111111-1111-4111-8111-111111111111',
      location: {
        providerId: 'local',
        uri: 'file:///documents/TRIZ%20Engineering.pdf',
      },
    } satisfies ResolvedRagCitation);
    const navigate = vi.fn().mockResolvedValue(undefined);

    await expect(
      navigateToRagCitation('22222222-2222-4222-8222-222222222222', 'source-1', resolve, navigate),
    ).resolves.toBe(true);

    expect(resolve).toHaveBeenCalledWith({
      workspaceId: '22222222-2222-4222-8222-222222222222',
      sourceId: 'source-1',
    });
    expect(navigate).toHaveBeenCalledWith(
      { providerId: 'local', uri: 'file:///documents' },
      'TRIZ Engineering.pdf',
    );
  });
});
