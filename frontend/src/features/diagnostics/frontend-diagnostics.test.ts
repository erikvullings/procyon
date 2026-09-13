import { afterEach, describe, expect, it, vi } from 'vitest';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { DiagnosticError } from './diagnostics';
import { installFrontendDiagnostics } from './frontend-diagnostics';

describe('installFrontendDiagnostics', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('reports uncaught errors, rejected promises, and console errors through the client', async () => {
    const recorded: DiagnosticError[] = [];
    const client = {
      recordFrontendDiagnostic: (error: DiagnosticError) => {
        recorded.push(error);
        return Promise.resolve();
      },
    } as FileManagerClient;
    vi.spyOn(console, 'error').mockImplementation(() => {});

    const diagnostics = installFrontendDiagnostics(client);
    window.dispatchEvent(
      new ErrorEvent('error', {
        message: 'render failed',
        error: new Error('render failed'),
      }),
    );
    const rejection = new Event('unhandledrejection');
    Object.defineProperty(rejection, 'reason', { value: new Error('request failed') });
    window.dispatchEvent(rejection);
    console.error('explicit failure', { status: 500 });

    await vi.waitFor(() => expect(recorded).toHaveLength(3));
    expect(recorded.map(({ code }) => code)).toEqual([
      'FRONTEND_UNCAUGHT_ERROR',
      'FRONTEND_UNHANDLED_REJECTION',
      'FRONTEND_CONSOLE_ERROR',
    ]);
    expect(recorded[2]?.message).toContain('explicit failure');

    diagnostics.uninstall();
  });

  it('restores console.error and removes listeners when uninstalled', async () => {
    const recordFrontendDiagnostic = vi.fn().mockResolvedValue(undefined);
    const client = { recordFrontendDiagnostic } as unknown as FileManagerClient;
    const originalConsoleError = console.error;

    const diagnostics = installFrontendDiagnostics(client);
    diagnostics.uninstall();
    window.dispatchEvent(new ErrorEvent('error', { message: 'after cleanup' }));

    expect(console.error).toBe(originalConsoleError);
    expect(recordFrontendDiagnostic).not.toHaveBeenCalled();
  });

  it('queues HTTP startup errors until the session gate is ready', async () => {
    const recordFrontendDiagnostic = vi.fn().mockResolvedValue(undefined);
    const client = { recordFrontendDiagnostic } as unknown as FileManagerClient;
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const diagnostics = installFrontendDiagnostics(client, { deferUntilReady: true });

    console.error('startup failure');
    expect(recordFrontendDiagnostic).not.toHaveBeenCalled();

    diagnostics.flush();
    await vi.waitFor(() => expect(recordFrontendDiagnostic).toHaveBeenCalledOnce());
    diagnostics.uninstall();
  });
});
