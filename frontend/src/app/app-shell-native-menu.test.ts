import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { NativeMenuItem, NativeMenuSpec } from '../models';

const invoke = vi.fn();

class MockChannel<T> {
  onmessage: (message: T) => void = () => undefined;
}

vi.mock('@tauri-apps/api/core', () => ({
  Channel: MockChannel,
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onDragDropEvent: vi.fn(),
    toggleMaximize: vi.fn(),
    startDragging: vi.fn(),
  }),
}));

const { MockFileManagerClient } = await import('../api/client/mock-file-manager-client');
const { AppShell } = await import('./app-shell');

let root: HTMLElement;

/** The most recent spec pushed to the desktop menu bar. */
function lastPushedMenuSpec(): NativeMenuSpec | undefined {
  const pushes = invoke.mock.calls.filter(([command]) => command === 'set_native_menu');
  return pushes.at(-1)?.[1]?.spec as NativeMenuSpec | undefined;
}

function menuItemIds(spec: NativeMenuSpec | undefined, title: string): string[] {
  const menu = spec?.menus.find((candidate) => candidate.title === title);
  return (menu?.items ?? [])
    .filter((item): item is Extract<NativeMenuItem, { kind: 'action' }> => item.kind === 'action')
    .map((item) => item.id);
}

beforeEach(() => {
  invoke.mockImplementation(() => Promise.resolve());
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  invoke.mockReset();
  vi.restoreAllMocks();
});

describe('AppShell native menu synchronisation (task 0206)', () => {
  it('offers the client-only Search Knowledge action in the desktop Tools menu', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });

    await vi.waitFor(() => {
      expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).toContain('client.searchKnowledge');
    });
    // The registry-backed items must still be there alongside it.
    expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).toContain('core.copyPath');
  });

  it('omits Search Knowledge when no retrieval capability is reported', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: false,
      semantic: false,
      answerGeneration: false,
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });

    await vi.waitFor(() => {
      expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).toContain('core.copyPath');
    });
    expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).not.toContain('client.searchKnowledge');
  });

  it('omits Search Knowledge when only answer generation is reported (task 0208)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getKnowledgeCapabilities').mockResolvedValue({
      fullText: false,
      semantic: false,
      answerGeneration: true,
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });

    await vi.waitFor(() => {
      expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).toContain('core.copyPath');
    });
    expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).not.toContain('client.searchKnowledge');
  });

  it('opens the knowledge search pane when the menu item is activated', async () => {
    const client = new MockFileManagerClient();
    let dispatch: ((message: { id: string }) => void) | undefined;
    invoke.mockImplementation((command: string, payload?: Record<string, unknown>) => {
      if (command === 'subscribe_native_menu_actions') {
        const channel = payload?.channel as MockChannel<{ id: string }> | undefined;
        dispatch = channel?.onmessage.bind(channel);
      }
      return Promise.resolve();
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });
    await vi.waitFor(() => {
      expect(menuItemIds(lastPushedMenuSpec(), 'Tools')).toContain('client.searchKnowledge');
    });

    dispatch?.({ id: 'client.searchKnowledge' });

    await vi.waitFor(() => {
      const search = root.querySelector('.fm-knowledge-search');
      expect(search).not.toBeNull();
      expect(search?.textContent).toContain('What are you looking for?');
    });
  });
});
