import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Connection, OneDriveAuthorizationAttempt } from '../../models';
import { ConnectionsManager, type ConnectionsManagerAttrs } from './connection-editor';

let root: HTMLElement;

function oneDriveConnection(overrides: Partial<Connection> = {}): Connection {
  return {
    id: '11111111-1111-4111-8111-111111111111',
    name: 'Work OneDrive',
    kind: 'oneDrive',
    configuration: {
      kind: 'oneDrive',
      accountHint: 'erik@example.test',
      displayName: null,
      email: null,
      driveType: null,
    },
    hasCredential: false,
    status: 'authenticationRequired',
    rootLocation: 'onedrive://11111111-1111-4111-8111-111111111111/',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
    ...overrides,
  };
}

function attrs(overrides: Partial<ConnectionsManagerAttrs> = {}): ConnectionsManagerAttrs {
  return {
    open: true,
    connections: [oneDriveConnection()],
    onRefresh: vi.fn().mockResolvedValue(undefined),
    onClose: vi.fn(),
    onSave: vi.fn(),
    onDelete: vi.fn(),
    onConnect: vi.fn(),
    onDisconnect: vi.fn(),
    onTest: vi.fn(),
    onProbeHostKey: vi.fn(),
    onAcceptHostKey: vi.fn(),
    onBeginOneDriveAuthorization: vi.fn().mockResolvedValue({
      attemptId: 'attempt-1',
      authorizationUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
    }),
    onGetOneDriveAuthorizationAttempt: vi.fn(),
    onCancelOneDriveAuthorization: vi.fn(),
    onOneDriveAuthorized: vi.fn(),
    ...overrides,
  };
}

function mount(componentAttrs: ConnectionsManagerAttrs): void {
  m.mount(root, { view: () => m(ConnectionsManager, componentAttrs) });
  m.redraw.sync();
}

function button(label: string): HTMLButtonElement {
  const found = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`button "${label}" not found`);
  return found;
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  m.redraw.sync();
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.useRealTimers();
});

describe('ConnectionsManager OneDrive authorization', () => {
  it('opens Microsoft sign-in, reports progress, and applies the authorized account', async () => {
    let resolveAttempt: ((attempt: OneDriveAuthorizationAttempt) => void) | undefined;
    const onGetOneDriveAuthorizationAttempt = vi.fn(
      () =>
        new Promise<OneDriveAuthorizationAttempt>((resolve) => {
          resolveAttempt = resolve;
        }),
    );
    const onOneDriveAuthorized = vi.fn();
    const componentAttrs = attrs({ onGetOneDriveAuthorizationAttempt, onOneDriveAuthorized });
    mount(componentAttrs);
    await flush();

    button('Sign in with Microsoft').click();
    await flush();

    expect(componentAttrs.onBeginOneDriveAuthorization).toHaveBeenCalledWith(
      '11111111-1111-4111-8111-111111111111',
    );
    expect(root.textContent).toContain('Waiting for Microsoft sign-in');

    const authorized = oneDriveConnection({
      hasCredential: true,
      status: 'connected',
      configuration: {
        kind: 'oneDrive',
        accountHint: 'erik@example.test',
        displayName: 'Erik Vullings',
        email: 'erik@example.test',
        driveType: 'business',
      },
    });
    resolveAttempt?.({
      id: 'attempt-1',
      status: { state: 'succeeded', connection: authorized },
    });
    await flush();

    expect(onOneDriveAuthorized).toHaveBeenCalledWith(authorized, false);
    expect(root.textContent).toContain('Microsoft account connected');
  });

  it('shows an actionable localized Conditional Access failure instead of the backend message', async () => {
    const componentAttrs = attrs({
      onGetOneDriveAuthorizationAttempt: vi.fn().mockResolvedValue({
        id: 'attempt-1',
        status: {
          state: 'failed',
          code: 'conditionalAccessRequired',
          message: 'raw backend policy text',
        },
      }),
    });
    mount(componentAttrs);
    await flush();

    button('Sign in with Microsoft').click();
    await flush();

    expect(root.textContent).toContain('Your organization requires another sign-in step');
    expect(root.textContent).not.toContain('raw backend policy text');
  });

  it('cancels a pending authorization attempt', async () => {
    const onGetOneDriveAuthorizationAttempt = vi.fn(
      () => new Promise<OneDriveAuthorizationAttempt>(() => undefined),
    );
    const onCancelOneDriveAuthorization = vi.fn().mockResolvedValue({
      id: 'attempt-1',
      status: { state: 'cancelled' },
    });
    const componentAttrs = attrs({
      onGetOneDriveAuthorizationAttempt,
      onCancelOneDriveAuthorization,
    });
    mount(componentAttrs);
    await flush();

    button('Sign in with Microsoft').click();
    await flush();
    button('Cancel sign-in').click();
    await flush();

    expect(onCancelOneDriveAuthorization).toHaveBeenCalledWith('attempt-1');
    expect(root.textContent).toContain('Microsoft sign-in cancelled');
  });

  it('clears and cancels a pending attempt when the manager closes', async () => {
    const onGetOneDriveAuthorizationAttempt = vi.fn(
      () => new Promise<OneDriveAuthorizationAttempt>(() => undefined),
    );
    const onCancelOneDriveAuthorization = vi.fn().mockResolvedValue({
      id: 'attempt-1',
      status: { state: 'cancelled' },
    });
    const componentAttrs = attrs({
      onGetOneDriveAuthorizationAttempt,
      onCancelOneDriveAuthorization,
    });
    mount(componentAttrs);
    await flush();
    button('Sign in with Microsoft').click();
    await flush();

    button('Close').click();
    await flush();

    expect(onCancelOneDriveAuthorization).toHaveBeenCalledWith('attempt-1');
    expect(root.textContent).not.toContain('Waiting for Microsoft sign-in');
    expect(button('Sign in with Microsoft')).toBeDefined();
  });

  it('cancels a begin result that arrives after the manager closes without polling it', async () => {
    let resolveBegin:
      | ((value: { readonly attemptId: string; readonly authorizationUrl: string }) => void)
      | undefined;
    const onBeginOneDriveAuthorization = vi.fn(
      () =>
        new Promise<{ readonly attemptId: string; readonly authorizationUrl: string }>(
          (resolve) => {
            resolveBegin = resolve;
          },
        ),
    );
    const onGetOneDriveAuthorizationAttempt = vi.fn();
    const onCancelOneDriveAuthorization = vi.fn().mockResolvedValue({
      id: 'attempt-late',
      status: { state: 'cancelled' },
    });
    const componentAttrs = attrs({
      onBeginOneDriveAuthorization,
      onGetOneDriveAuthorizationAttempt,
      onCancelOneDriveAuthorization,
    });
    mount(componentAttrs);
    await flush();
    button('Sign in with Microsoft').click();
    button('Close').click();

    resolveBegin?.({
      attemptId: 'attempt-late',
      authorizationUrl: 'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
    });
    await flush();

    expect(onCancelOneDriveAuthorization).toHaveBeenCalledWith('attempt-late');
    expect(onGetOneDriveAuthorizationAttempt).not.toHaveBeenCalled();
    expect(root.textContent).not.toContain('Waiting for Microsoft sign-in');
  });
});
