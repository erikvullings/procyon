import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TerminalClient } from './terminal-client';
import { handleTerminalKeyEvent, showTerminalSurface, TerminalDrawer } from './terminal-drawer';

describe('TerminalDrawer', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.append(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  it('renders a terminal location without mixing keyed and unkeyed children', () => {
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write: vi.fn(async () => undefined),
      resize: vi.fn(async () => undefined),
    };

    expect(() =>
      m.mount(root, {
        view: () =>
          m(TerminalDrawer, {
            open: false,
            tabKey: 'pane-1:tab-1',
            location: { providerId: 'local', uri: 'file:///home' },
            client,
            onToggle: vi.fn(),
          }),
      }),
    ).not.toThrow();
  });

  it('opens one PTY while terminal startup is still pending across redraws', () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    let resolveOpen: ((sessionId: string) => void) | undefined;
    const client: TerminalClient = {
      open: vi.fn(
        () =>
          new Promise<string>((resolve) => {
            resolveOpen = resolve;
          }),
      ),
      write: vi.fn(async () => undefined),
      resize: vi.fn(async () => undefined),
    };
    const location = { providerId: 'local', uri: 'file:///home' };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location,
          client,
          onToggle: vi.fn(),
        }),
    });

    m.redraw.sync();
    m.redraw.sync();

    expect(client.open).toHaveBeenCalledOnce();
    resolveOpen?.('session-1');
    vi.unstubAllGlobals();
  });

  it('returns F12 to the file manager while xterm has focus', () => {
    const toggle = vi.fn();
    const event = new KeyboardEvent('keydown', { key: 'F12', cancelable: true });
    const stopPropagation = vi.spyOn(event, 'stopPropagation');

    expect(handleTerminalKeyEvent(event, { onToggle: toggle })).toBe(false);
    expect(event.defaultPrevented).toBe(true);
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(toggle).toHaveBeenCalledOnce();
  });

  it('returns Ctrl+backtick to the file manager while xterm has focus', () => {
    const toggle = vi.fn();
    const event = new KeyboardEvent('keydown', {
      key: 'Dead',
      code: 'Backquote',
      ctrlKey: true,
      cancelable: true,
    });

    expect(handleTerminalKeyEvent(event, { onToggle: toggle })).toBe(false);
    expect(toggle).toHaveBeenCalledOnce();
  });

  it('leaves plain Tab and Shift+Tab for the shell to use as completion keys', () => {
    const switchPane = vi.fn();
    const cycleTab = vi.fn();
    const focusFolder = vi.fn();
    const handlers = {
      onToggle: vi.fn(),
      onSwitchPane: switchPane,
      onCycleTab: cycleTab,
      onFocusFolder: focusFolder,
    };

    expect(
      handleTerminalKeyEvent(
        new KeyboardEvent('keydown', { key: 'Tab', cancelable: true }),
        handlers,
      ),
    ).toBe(true);
    expect(
      handleTerminalKeyEvent(
        new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, cancelable: true }),
        handlers,
      ),
    ).toBe(true);

    expect(switchPane).not.toHaveBeenCalled();
    expect(cycleTab).not.toHaveBeenCalled();
    expect(focusFolder).not.toHaveBeenCalled();
  });

  it('returns modified Tab chords to the file manager for pane/tab navigation', () => {
    const switchPane = vi.fn();
    const cycleTab = vi.fn();
    const focusFolder = vi.fn();
    const handlers = {
      onToggle: vi.fn(),
      onSwitchPane: switchPane,
      onCycleTab: cycleTab,
      onFocusFolder: focusFolder,
    };

    expect(
      handleTerminalKeyEvent(
        new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, cancelable: true }),
        handlers,
      ),
    ).toBe(false);
    expect(
      handleTerminalKeyEvent(
        new KeyboardEvent('keydown', { key: 'Tab', altKey: true, cancelable: true }),
        handlers,
      ),
    ).toBe(false);
    expect(
      handleTerminalKeyEvent(
        new KeyboardEvent('keydown', {
          key: 'Tab',
          altKey: true,
          shiftKey: true,
          cancelable: true,
        }),
        handlers,
      ),
    ).toBe(false);

    expect(cycleTab).toHaveBeenCalledExactlyOnceWith(1);
    expect(switchPane).toHaveBeenCalledOnce();
    expect(focusFolder).toHaveBeenCalledOnce();
  });

  it('restores each folder terminal after another folder terminal used the drawer', () => {
    const drawerHost = document.createElement('div');
    const folder2Terminal = document.createElement('div');
    folder2Terminal.dataset.location = 'folder-2';
    const folder3Terminal = document.createElement('div');
    folder3Terminal.dataset.location = 'folder-3';

    showTerminalSurface(drawerHost, folder2Terminal);
    showTerminalSurface(drawerHost, folder3Terminal);
    showTerminalSurface(drawerHost, folder2Terminal);

    expect(drawerHost.firstElementChild).toBe(folder2Terminal);
  });
});
