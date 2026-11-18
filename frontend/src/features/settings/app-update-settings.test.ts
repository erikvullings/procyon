import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConnectionClient } from '../../api/client/connection-client';
import { AppUpdateSettings, isPrereleaseVersion } from './app-update-settings';

function stubClient(version: string): ConnectionClient {
  return {
    supportsAppUpdates: true,
    supportsAppDataRemoval: false,
    getAppVersion: vi.fn().mockResolvedValue(version),
    checkForAppUpdate: vi
      .fn()
      .mockResolvedValue({ currentVersion: version, latestVersion: version }),
  } as unknown as ConnectionClient;
}

describe('isPrereleaseVersion', () => {
  it('treats a version with a pre-release suffix as a pre-release', () => {
    expect(isPrereleaseVersion('0.7.0-16')).toBe(true);
    expect(isPrereleaseVersion('0.7.0')).toBe(false);
    expect(isPrereleaseVersion(undefined)).toBe(false);
  });
});

describe('AppUpdateSettings beta switch', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  async function mountWith(version: string, includeBeta: boolean, onIncludeBetaChange = vi.fn()) {
    const client = stubClient(version);
    m.mount(root, {
      view: () =>
        m(AppUpdateSettings, {
          client,
          automaticChecks: true,
          onAutomaticChecksChange: vi.fn(),
          includeBeta,
          onIncludeBetaChange,
        }),
    });
    m.redraw.sync();
    await vi.waitFor(() => expect(root.textContent).toContain(version));
    m.redraw.sync();
    const beta = root.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')[1];
    if (beta === undefined) throw new Error('beta switch not rendered');
    return { client, beta, onIncludeBetaChange };
  }

  it('shows the switch on and locked for a pre-release build', async () => {
    const { beta } = await mountWith('0.7.0-16', false);
    expect(beta.checked).toBe(true);
    expect(beta.disabled).toBe(true);
    expect(root.textContent).toContain('always checked');
  });

  it('lets a release build opt in to beta releases', async () => {
    const onIncludeBetaChange = vi.fn();
    const { beta } = await mountWith('0.7.0', false, onIncludeBetaChange);
    expect(beta.checked).toBe(false);
    expect(beta.disabled).toBe(false);
    beta.click();
    expect(onIncludeBetaChange).toHaveBeenCalledWith(true);
  });

  it('keeps the check action available on a pre-release build', async () => {
    const { client } = await mountWith('0.7.0-16', false);
    root.querySelector<HTMLButtonElement>('.fm-update-check')?.click();
    await vi.waitFor(() =>
      expect(client.checkForAppUpdate).toHaveBeenCalledWith(undefined, { includeBeta: true }),
    );
  });
});
