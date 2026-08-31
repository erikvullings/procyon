import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { fetchMutator } from '../api/fetch-mutator';
import { getSessionToken, resetSessionTokenForTests } from '../api/session-token';
import { SessionTokenGate } from './session-token-gate';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
  resetSessionTokenForTests();
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  resetSessionTokenForTests();
  vi.restoreAllMocks();
});

function mountGate(childText = 'gated content') {
  m.mount(root, {
    view: () => m(SessionTokenGate, { children: () => m('div', childText) }),
  });
  m.redraw.sync();
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('SessionTokenGate', () => {
  it('renders nothing while probing whether the server requires a token', () => {
    vi.stubGlobal('fetch', vi.fn().mockReturnValue(new Promise(() => undefined)));
    mountGate();

    expect(root.textContent).toBe('');
  });

  it('steps aside and renders the gated content when the probe succeeds unauthenticated (dev mode)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(200, { runtimeKind: 'browserServer' })),
    );
    mountGate();

    await vi.waitFor(() => expect(root.textContent).toContain('gated content'));
  });

  it('prompts for a token when the probe is rejected as unauthorized', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(401, { code: 'unauthorized', message: 'nope' })),
    );
    mountGate();

    await vi.waitFor(() => expect(root.textContent).toContain('Sign in to fm-server'));
    expect(root.textContent).not.toContain('gated content');
  });

  it('renders the gated content immediately, without probing, when a token is already stored', () => {
    const fetchSpy = vi.fn().mockResolvedValue(jsonResponse(200, {}));
    vi.stubGlobal('fetch', fetchSpy);
    sessionStorage.setItem('fm.sessionToken', 'existing-token');

    mountGate();

    expect(root.textContent).toContain('gated content');
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('stores the entered token and reveals the gated content on submit', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(401, { code: 'unauthorized', message: 'nope' })),
    );
    mountGate();
    await vi.waitFor(() => expect(root.textContent).toContain('Sign in to fm-server'));

    const input = root.querySelector('input') as HTMLInputElement;
    input.value = 'pasted-token';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    root
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    m.redraw.sync();

    expect(getSessionToken()).toBe('pasted-token');
    expect(root.textContent).toContain('gated content');
  });

  it('does not submit an empty or whitespace-only token', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(401, { code: 'unauthorized', message: 'nope' })),
    );
    mountGate();
    await vi.waitFor(() => expect(root.textContent).toContain('Sign in to fm-server'));

    root
      .querySelector('form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    m.redraw.sync();

    expect(getSessionToken()).toBeUndefined();
    expect(root.textContent).toContain('Sign in to fm-server');
  });

  it('clears the token and re-prompts with an error after a 401 response once authenticated', async () => {
    sessionStorage.setItem('fm.sessionToken', 'stale-token');
    mountGate();
    expect(root.textContent).toContain('gated content');

    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(401, { code: 'unauthorized', message: 'nope' })),
    );
    await fetchMutator('/api/v1/workspaces', {}).catch(() => undefined);
    m.redraw.sync();

    expect(getSessionToken()).toBeUndefined();
    expect(root.textContent).toContain('Sign in to fm-server');
    expect(root.textContent).toContain('rejected that token');
  });
});
