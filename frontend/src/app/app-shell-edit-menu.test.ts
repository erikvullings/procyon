import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../api/client/mock-file-manager-client';
import { EDIT_MENU_IDS } from '../features/native-menu/native-menu-dispatch';
import { FILE_CLIPBOARD_PASSTHROUGH_ATTRIBUTE } from '../features/quick-filter/quick-filter';

// The macOS menu bar owns Cmd+X/C/V/A as key equivalents, so in the desktop app those chords
// arrive through the native-menu channel rather than as keydowns. These tests drive the whole
// chain from that channel: Tauri channel -> dispatchNativeMenuAction -> replayed keydown ->
// global keydown routes -> file clipboard -> operation engine.
const tauri = vi.hoisted(() => {
  const state: {
    menuChannel: { onmessage: (event: { id: string }) => void } | undefined;
    invoke: ReturnType<typeof vi.fn<(command: string, args?: unknown) => Promise<unknown>>>;
  } = { menuChannel: undefined, invoke: vi.fn() };
  return state;
});

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: (event: { id: string }) => void = () => undefined;
  },
  invoke: (command: string, args?: { channel?: unknown }) => {
    if (command === 'subscribe_native_menu_actions')
      tauri.menuChannel = args?.channel as typeof tauri.menuChannel;
    return tauri.invoke(command, args);
  },
}));

import { AppShell } from './app-shell';

let root: HTMLElement;

class TestEventSource extends EventTarget {
  close(): void {}
}

beforeEach(() => {
  vi.stubGlobal('EventSource', TestEventSource);
  tauri.menuChannel = undefined;
  tauri.invoke.mockReset();
  tauri.invoke.mockResolvedValue(undefined);
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  vi.unstubAllGlobals();
  m.mount(root, null);
  root.remove();
});

async function mountMacDesktop() {
  const client = new MockFileManagerClient();
  vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
    ...(await client.getRuntimeCapabilities()),
    platform: 'macos',
    runtime: 'tauri',
  });
  const startOperation = vi.spyOn(client, 'startOperation');
  m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });
  await vi.waitFor(() => expect(root.textContent).toContain('.env'));
  await vi.waitFor(() => expect(tauri.menuChannel).toBeDefined());
  return {
    startOperation,
    menu: (id: string) => tauri.menuChannel?.onmessage({ id }),
  };
}

function activePane(): HTMLElement | null {
  return root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
}

function nativeEditCommands(): unknown[] {
  return tauri.invoke.mock.calls
    .filter(([command]) => command === 'perform_native_edit_command')
    .map(([, args]) => args);
}

describe('macOS Edit menu', () => {
  it('copies the cursor entry from the native Copy item and pastes it with Paste', async () => {
    const { startOperation, menu } = await mountMacDesktop();
    activePane()?.focus();

    menu(EDIT_MENU_IDS.copy);
    menu(EDIT_MENU_IDS.paste);

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'duplicate',
      sources: [{ uri: 'mock:///Applications' }],
      conflictPolicy: 'ask',
    });
    expect(nativeEditCommands()).toEqual([]);
  });

  it('copies a marked selection from the native Copy item', async () => {
    const { startOperation, menu } = await mountMacDesktop();
    const pane = activePane();
    pane?.focus();
    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    m.redraw.sync();
    expect(pane?.querySelectorAll('.fm-selected-row')).toHaveLength(2);

    menu(EDIT_MENU_IDS.copy);
    menu(EDIT_MENU_IDS.paste);

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]?.sources).toHaveLength(2);
    expect(nativeEditCommands()).toEqual([]);
  });

  it('leaves text-field Copy and Paste to the native responder chain', async () => {
    const { startOperation, menu } = await mountMacDesktop();
    const input = document.createElement('input');
    root.appendChild(input);
    input.focus();

    menu(EDIT_MENU_IDS.copy);
    menu(EDIT_MENU_IDS.paste);
    menu(EDIT_MENU_IDS.selectAll);

    expect(nativeEditCommands()).toEqual([
      { command: 'copy' },
      { command: 'paste' },
      { command: 'selectAll' },
    ]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(startOperation).not.toHaveBeenCalled();
  });

  it('copies files from the quick filter when it has no text selection', async () => {
    const { startOperation, menu } = await mountMacDesktop();
    const filter = document.createElement('input');
    filter.setAttribute(FILE_CLIPBOARD_PASSTHROUGH_ATTRIBUTE, '');
    root.appendChild(filter);
    filter.focus();

    menu(EDIT_MENU_IDS.copy);
    activePane()?.focus();
    menu(EDIT_MENU_IDS.paste);

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'duplicate',
      sources: [{ uri: 'mock:///Applications' }],
    });
    expect(nativeEditCommands()).toEqual([]);
  });

  it('selects every entry from the native Select All item while a pane has focus', async () => {
    const { menu } = await mountMacDesktop();
    const pane = activePane();
    pane?.focus();

    menu(EDIT_MENU_IDS.selectAll);
    m.redraw.sync();

    const rows = pane?.querySelectorAll('.fm-directory-row').length ?? 0;
    expect(rows).toBeGreaterThan(1);
    expect(pane?.querySelectorAll('.fm-selected-row')).toHaveLength(rows);
    expect(nativeEditCommands()).toEqual([]);
  });
});

describe('browser Cmd+C/Cmd+V keydowns', () => {
  it('copies a marked selection with Cmd+C and pastes it with Cmd+V', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      ...(await client.getRuntimeCapabilities()),
      platform: 'macos',
    });
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const pane = activePane();
    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    m.redraw.sync();

    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', metaKey: true, bubbles: true }));
    pane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'v', metaKey: true, bubbles: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({ type: 'duplicate' });
    expect(startOperation.mock.calls[0]?.[0]?.sources).toHaveLength(2);
  });
});
