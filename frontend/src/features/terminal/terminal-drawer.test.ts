import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LlmProfile } from '../../models';
import type { TerminalAssistEvent } from './terminal-assist';
import type { TerminalClient } from './terminal-client';
import {
  handleTerminalKeyEvent,
  lineEditSequence,
  questionAtPrompt,
  showTerminalSurface,
  TerminalDrawer,
  type TerminalDrawerAttrs,
  type TerminalModels,
  terminalHost,
} from './terminal-drawer';

/** The chat input `#` opens, once drawn; `null` while it is closed. */
function chatInput(root: ParentNode): HTMLTextAreaElement | null {
  m.redraw.sync();
  return root.querySelector<HTMLTextAreaElement>('.fm-terminal-composer-input');
}

/** Where keys go: the chat input while it has focus, the terminal otherwise. */
function keyTarget(root: ParentNode, terminal: HTMLTextAreaElement): HTMLTextAreaElement {
  const input = chatInput(root);
  return input !== null && document.activeElement === input ? input : terminal;
}

/** Types `text` into the chat input, as the browser does. */
function typeInChat(input: HTMLTextAreaElement, text: string): void {
  input.value += text;
  input.setSelectionRange(input.value.length, input.value.length);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

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
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
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
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
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

  it('names the machine a location belongs to', () => {
    expect(terminalHost({ providerId: 'local', uri: 'file:///home' })).toBe('local');
    expect(terminalHost({ providerId: 'sftp', uri: 'sftp://abc-123/srv/app' })).toBe(
      'sftp://abc-123',
    );
  });

  it('opens a new session when the tab moves from a local folder to an SSH connection', async () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    const client: TerminalClient = {
      open: vi.fn(async () => 'session'),
      write: vi.fn(async () => undefined),
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
    };
    let location = { providerId: 'local', uri: 'file:///home' };
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
    await vi.waitFor(() => expect(client.open).toHaveBeenCalledTimes(1));

    location = { providerId: 'local', uri: 'file:///home/sub' };
    m.redraw.sync();
    expect(client.open).toHaveBeenCalledTimes(1);

    location = { providerId: 'sftp', uri: 'sftp://abc-123/srv' };
    m.redraw.sync();
    await vi.waitFor(() => expect(client.open).toHaveBeenCalledTimes(2));
    expect(vi.mocked(client.open).mock.calls[1]?.[0].uri).toBe('sftp://abc-123/srv');
    vi.unstubAllGlobals();
  });

  it('forwards editing and history keys from the focused xterm textarea to the PTY', async () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
        }),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await Promise.resolve();
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    textarea.focus();

    for (const [key, keyCode] of [
      ['ArrowLeft', 37],
      ['ArrowRight', 39],
      ['ArrowUp', 38],
      ['ArrowDown', 40],
      ['Backspace', 8],
    ] as const) {
      const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      Object.defineProperty(event, 'keyCode', { value: keyCode });
      textarea.dispatchEvent(event);
    }

    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(5));
    expect(write.mock.calls.map(([, data]) => [...data])).toEqual([
      [27, 91, 68],
      [27, 91, 67],
      [27, 91, 65],
      [27, 91, 66],
      [127],
    ]);
    vi.unstubAllGlobals();
  });

  it('forwards pasted text from xterm to the PTY', async () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
        }),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await Promise.resolve();
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', {
      value: { getData: (type: string) => (type === 'text/plain' ? 'git status' : '') },
    });

    textarea.dispatchEvent(event);

    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    const writeCall = write.mock.calls[0];
    if (writeCall === undefined) throw new Error('pasted text was not forwarded');
    expect([...writeCall[1]]).toEqual([...new TextEncoder().encode('git status')]);
    vi.unstubAllGlobals();
  });

  it('turns word and line chords into the shell bytes native terminals send', () => {
    const seq = (init: KeyboardEventInit) => lineEditSequence(new KeyboardEvent('keydown', init));
    expect(seq({ key: 'ArrowLeft', altKey: true })).toBe('\x1bb');
    expect(seq({ key: 'ArrowRight', ctrlKey: true })).toBe('\x1bf');
    expect(seq({ key: 'ArrowLeft', metaKey: true })).toBe('\x01');
    expect(seq({ key: 'ArrowRight', metaKey: true })).toBe('\x05');
    expect(seq({ key: 'Backspace', altKey: true })).toBe('\x1b\x7f');
    expect(seq({ key: 'Backspace', ctrlKey: true })).toBe('\x17');
    expect(seq({ key: 'Backspace', metaKey: true })).toBe('\x15');
    expect(seq({ key: 'Delete', altKey: true })).toBe('\x1bd');
    expect(seq({ key: 'Delete', metaKey: true })).toBe('\x0b');
    expect(seq({ key: 'ArrowLeft' })).toBeUndefined();
    expect(seq({ key: 'ArrowLeft', altKey: true, shiftKey: true })).toBeUndefined();
    expect(seq({ key: 'a', altKey: true })).toBeUndefined();
  });

  it('sends a line-edit chord to the shell and keeps it from xterm', () => {
    const onShellInput = vi.fn();
    const event = new KeyboardEvent('keydown', {
      key: 'Backspace',
      altKey: true,
      cancelable: true,
    });
    expect(handleTerminalKeyEvent(event, { onToggle: vi.fn(), onShellInput })).toBe(false);
    expect(onShellInput).toHaveBeenCalledWith('\x1b\x7f');
    expect(event.defaultPrevented).toBe(true);
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

describe('TerminalDrawer questions with #', () => {
  let root: HTMLElement;

  beforeEach(() => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  function mount(client: TerminalClient): void {
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
        }),
    });
  }

  function pasteText(textarea: HTMLTextAreaElement, text: string): void {
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', {
      value: { getData: (type: string) => (type === 'text/plain' ? text : '') },
    });
    textarea.dispatchEvent(event);
  }

  function pressEnter(textarea: HTMLTextAreaElement): void {
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(event, 'keyCode', { value: 13 });
    keyTarget(root, textarea).dispatchEvent(event);
  }

  /** Waits for `#` to open the chat input with `text` in it. */
  async function chatHolds(text: string): Promise<void> {
    await vi.waitFor(() => expect(chatInput(root)?.value).toBe(text));
  }

  it('sends a # question from the chat input, and the shell never sees it', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const assist = vi.fn(async () => ({ text: 'Try this.\nCommand: ls -la' }));
    mount({
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist,
      assistClose: vi.fn(async () => undefined),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');

    pasteText(textarea, '#list files');
    await chatHolds('list files');
    expect(write).not.toHaveBeenCalled();
    pressEnter(textarea);

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls -la'),
    );
    expect(assist).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: 'ask',
        request: 'list files',
        workingDirectory: 'file:///home',
      }),
      expect.any(Function),
    );
    // The shell never sees the question; it only gets Enter for a fresh prompt after the answer.
    const sent = write.mock.calls.map(([, data]) => new TextDecoder().decode(data));
    expect(sent.join('')).not.toContain('list files');
    expect(sent).toContain('\r');
    expect(sent).not.toContain('\n');
  });

  it('inserts an answer at the prompt without running it', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    mount({
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: 'Try this.\nCommand: ls -la' })),
      assistClose: vi.fn(async () => undefined),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    pasteText(textarea, '#list files');
    await chatHolds('list files');
    expect(write).not.toHaveBeenCalled();
    pressEnter(textarea);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls -la'),
    );

    const insert = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.startsWith('Insert at prompt') === true,
    );
    insert?.click();

    await vi.waitFor(() =>
      expect(write).toHaveBeenLastCalledWith('session-1', new TextEncoder().encode('ls -la')),
    );
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-command')).toBeNull());
  });
});

describe('TerminalDrawer consent prompt', () => {
  let root: HTMLElement;

  beforeEach(() => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  function consentButton(label: string): HTMLButtonElement | undefined {
    return [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.startsWith(label) === true,
    );
  }

  it('goes away once the choice is saved', async () => {
    let choice: 'unset' | 'enabled' | 'disabled' = 'unset';
    const save = vi.fn(async (next: 'enabled' | 'disabled') => {
      choice = next;
    });
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write: vi.fn(async () => undefined),
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: '' })),
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
          shellIntegration: choice,
          onShellIntegrationChoice: save,
        }),
    });
    await vi.waitFor(() => expect(consentButton('Enable')).toBeDefined());

    consentButton('Enable')?.click();

    await vi.waitFor(() => expect(save).toHaveBeenCalledWith('enabled'));
    await vi.waitFor(() => expect(consentButton('Enable')).toBeUndefined());
    expect(root.querySelector('.fm-terminal-consent')).toBeNull();
  });
});

describe('TerminalDrawer after inserting a generated command', () => {
  let root: HTMLElement;

  beforeEach(() => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  it('still forwards Enter and Ctrl+C to the shell', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: 'Try this.\nCommand: ls -la' })),
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
          shellIntegration: 'disabled',
        }),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', {
      value: { getData: () => '#list files' },
    });
    textarea.dispatchEvent(paste);
    await vi.waitFor(() => expect(chatInput(root)?.value).toBe('list files'));
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(enter, 'keyCode', { value: 13 });
    keyTarget(root, textarea).dispatchEvent(enter);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls -la'),
    );
    const insert = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.startsWith('Insert at prompt') === true,
    );
    insert?.click();
    // Enter for a fresh prompt after the answer, then the inserted command.
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(2));

    const submit = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(submit, 'keyCode', { value: 13 });
    textarea.dispatchEvent(submit);
    const interrupt = new KeyboardEvent('keydown', {
      key: 'c',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(interrupt, 'keyCode', { value: 67 });
    textarea.dispatchEvent(interrupt);

    await vi.waitFor(() =>
      expect(write.mock.calls.map(([, data]) => [...data])).toContainEqual([3]),
    );
  });
});

describe('TerminalDrawer # after editing keys', () => {
  let root: HTMLElement;

  beforeEach(() => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  it('opens the chat input when the screen shows an empty prompt after an arrow key', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const assist = vi.fn(async () => ({ text: 'Try this.\nCommand: ls -la' }));
    let echo: ((event: { type: 'output'; data: number[] }) => void) | undefined;
    const client: TerminalClient = {
      open: vi.fn(async (_location, _cols, _rows, output) => {
        echo = output;
        return 'session-1';
      }),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist,
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
          shellIntegration: 'disabled',
        }),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    const key = (name: string, keyCode: number) => {
      const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true });
      Object.defineProperty(event, 'keyCode', { value: keyCode });
      keyTarget(root, textarea).dispatchEvent(event);
    };
    const paste = (text: string) => {
      const event = new Event('paste', { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'clipboardData', { value: { getData: () => text } });
      textarea.dispatchEvent(event);
      // The shell echoes what was typed onto the prompt line.
      echo?.({ type: 'output', data: [...new TextEncoder().encode(`user % ${text}`)] });
    };

    await vi.waitFor(() => expect(echo).toBeDefined());
    key('ArrowUp', 38);
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    paste('# How to list all files sorted by size');
    await vi.waitFor(() =>
      expect(chatInput(root)?.value).toBe('How to list all files sorted by size'),
    );
    expect(write).toHaveBeenCalledOnce();
    key('Enter', 13);

    await vi.waitFor(() =>
      expect(assist).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: 'ask',
          request: 'How to list all files sorted by size',
        }),
        expect.any(Function),
      ),
    );
    // Only Enter follows, to bring the prompt back after the answer.
    await vi.waitFor(() => expect(write.mock.calls.map(([, data]) => data[0])).toContain(13));
  });
});

describe('questionAtPrompt', () => {
  it('reads a question from the tracked input when it is known', () => {
    expect(questionAtPrompt({ text: '# list files', known: true }, '')).toBe('list files');
    expect(questionAtPrompt({ text: '#xxx', known: true }, '')).toBe('xxx');
    expect(questionAtPrompt({ text: '', known: false }, 'user % #xxx')).toBe('xxx');
  });

  it('reads a question from the screen line when the tracked input is unknown', () => {
    expect(questionAtPrompt({ text: '', known: false }, 'user % # list files')).toBe('list files');
    expect(questionAtPrompt({ text: '', known: false }, '# list files')).toBe('list files');
  });

  it('does not take a # comment after a command as a question', () => {
    expect(questionAtPrompt({ text: '', known: false }, 'user % ls # list files')).toBeUndefined();
    expect(questionAtPrompt({ text: 'ls # x', known: true }, '')).toBeUndefined();
  });
});

describe('TerminalDrawer keyboard shortcuts for answers', () => {
  let root: HTMLElement;
  let write: ReturnType<typeof vi.fn<TerminalClient['write']>>;

  beforeEach(async () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
    write = vi.fn<TerminalClient['write']>(async () => undefined);
    const client: TerminalClient = {
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: 'Try this.\nCommand: ls -la' })),
      assistClose: vi.fn(async () => undefined),
    };
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client,
          onToggle: vi.fn(),
          shellIntegration: 'disabled',
        }),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  function textarea(): HTMLTextAreaElement {
    const element = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (element === null) throw new Error('xterm textarea missing');
    return element;
  }

  function press(target: EventTarget, key: string, init: KeyboardEventInit = {}, keyCode = 0) {
    const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
    if (keyCode !== 0) Object.defineProperty(event, 'keyCode', { value: keyCode });
    target.dispatchEvent(event);
  }

  function sent(): number[][] {
    return write.mock.calls.map(([, data]) => [...data]);
  }

  async function askForAnswer(): Promise<void> {
    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => '#list files' } });
    textarea().dispatchEvent(paste);
    await vi.waitFor(() => expect(chatInput(root)?.value).toBe('list files'));
    press(keyTarget(root, textarea()), 'Enter', {}, 13);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls -la'),
    );
  }

  it('inserts a shown answer with Enter, without sending Enter to the shell', async () => {
    await askForAnswer();
    press(textarea(), 'Enter', {}, 13);

    await vi.waitFor(() => expect(sent()).toContainEqual([...new TextEncoder().encode('ls -la')]));
    const inserted = sent().findIndex((bytes) => bytes.length === 6);
    expect(
      sent()
        .slice(inserted + 1)
        .some(([byte]) => byte === 13),
    ).toBe(false);
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-command')).toBeNull());
  });

  it('rejects a shown answer with Ctrl+C, and sends Ctrl+C to the shell when no answer is shown', async () => {
    await askForAnswer();
    press(textarea(), 'c', { ctrlKey: true }, 67);
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-command')).toBeNull());
    expect(sent().some(([byte]) => byte === 3)).toBe(false);

    press(textarea(), 'c', { ctrlKey: true }, 67);
    await vi.waitFor(() => expect(sent()).toContainEqual([3]));
  });

  it('edits a shown answer with Ctrl+E before inserting it', async () => {
    await askForAnswer();
    press(textarea(), 'e', { ctrlKey: true });

    const edit = await vi.waitFor(() => {
      const element = root.querySelector<HTMLInputElement>('.fm-terminal-assist-edit');
      if (element === null) throw new Error('edit field missing');
      return element;
    });
    expect(edit.value).toBe('ls -la');
    edit.value = 'ls -lh';
    edit.dispatchEvent(new Event('input'));
    press(edit, 'Enter');

    await vi.waitFor(() => expect(sent()).toContainEqual([...new TextEncoder().encode('ls -lh')]));
  });
});

describe('TerminalDrawer assistant session (task 0336)', () => {
  let root: HTMLElement;
  let write: ReturnType<typeof vi.fn<TerminalClient['write']>>;
  let assist: ReturnType<typeof vi.fn<TerminalClient['assist']>>;
  let assistClose: ReturnType<typeof vi.fn<TerminalClient['assistClose']>>;
  let echo: ((event: { type: 'output'; data: number[] }) => void) | undefined;
  let disposeTab: ((tabKey: string) => void) | undefined;
  let paneListing: string[] = ['a.txt', 'src/'];
  let assistantEnabled = true;
  let autoRunReadOnly = true;
  let folderNames: readonly string[] = ['README.md'];
  const saveNote = vi.fn();

  const bytes = (text: string) => [...new TextEncoder().encode(text)];
  const textarea = () => {
    const element = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (element === null) throw new Error('xterm textarea missing');
    return element;
  };
  const key = (name: string, keyCode: number) => {
    const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true });
    Object.defineProperty(event, 'keyCode', { value: keyCode });
    keyTarget(root, textarea()).dispatchEvent(event);
  };
  /** Types `text` where the keys go: `#...` at the prompt opens the chat input. */
  const typeLine = (text: string) => {
    const target = keyTarget(root, textarea());
    if (target !== textarea()) {
      typeInChat(target, text.replace(/^#\s*/, ''));
      return;
    }
    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => text } });
    textarea().dispatchEvent(paste);
  };
  const ask = (text: string) => {
    typeLine(text);
    key('Enter', 13);
  };
  /** Shows a shell prompt mark and waits until the terminal has processed it. */
  const markPrompt = async () => {
    echo?.({ type: 'output', data: bytes('\x1b]133;A\x07user % ') });
    await new Promise((resolve) => setTimeout(resolve, 30));
  };
  const button = (label: string) =>
    [...root.querySelectorAll<HTMLButtonElement>('button')].find((candidate) =>
      candidate.textContent?.startsWith(label),
    );
  const sentAt = (call: number) => assist.mock.calls[call]?.[0];

  async function mount(extra: Partial<TerminalDrawerAttrs> = {}): Promise<void> {
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          onSaveAsNote: saveNote,
          ...extra,
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home' },
          client: {
            open: vi.fn(async (_location, _cols, _rows, output) => {
              echo = output;
              return 'session-1';
            }),
            write,
            resize: vi.fn(async () => undefined),
            close: vi.fn(async () => true),
            assist,
            assistClose,
          },
          onToggle: vi.fn(),
          shellIntegration: 'disabled',
          assistantEnabled,
          autoRunReadOnly,
          listFolder: async () => folderNames,
          paneContext: () => ({
            listing: paneListing,
            selected: ['a.txt'],
            otherFolder: 'file:///other',
          }),
          registerDisposeTab: (dispose) => {
            disposeTab = dispose;
          },
        }),
    });
    await vi.waitFor(() => expect(root.querySelector('.xterm-helper-textarea')).not.toBeNull());
    await vi.waitFor(() => expect(echo).toBeDefined());
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  beforeEach(() => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn() })),
    );
    root = document.createElement('div');
    document.body.append(root);
    echo = undefined;
    disposeTab = undefined;
    paneListing = ['a.txt', 'src/'];
    assistantEnabled = true;
    autoRunReadOnly = true;
    write = vi.fn<TerminalClient['write']>(async () => undefined);
    assist = vi.fn<TerminalClient['assist']>(async () => ({
      text: 'Fine.\nCommand: ls',
      steps: ['Read a.txt'],
    }));
    assistClose = vi.fn<TerminalClient['assistClose']>(async () => undefined);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    vi.unstubAllGlobals();
  });

  it('sends the pane context with a # question, writes the answer into the terminal, and keeps one session', async () => {
    await mount();
    ask('# what is here?');

    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)).toMatchObject({
      kind: 'ask',
      request: 'what is here?',
      workingDirectory: 'file:///home',
      context: { listing: ['a.txt', 'src/'], selected: ['a.txt'] },
    });
    expect(sentAt(0)?.context?.otherFolder).toBeUndefined();
    await vi.waitFor(() => {
      expect(root.textContent).toContain('what is here?');
      expect(root.textContent).toContain('Read a.txt');
      expect(root.textContent).toContain('Fine.');
    });

    ask('# and then?');

    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.request).toBe('and then?');
    expect(sentAt(1)?.sessionId).toBe(sentAt(0)?.sessionId);
    expect(sentAt(0)?.sessionId).toEqual(expect.any(String));
  });

  it('leaves a part out of the next question after its chip is toggled off', async () => {
    await mount();
    ask('# first');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(button('2 names')).toBeDefined());

    button('2 names')?.click();
    await vi.waitFor(() => expect(button('2 names')?.getAttribute('aria-pressed')).toBe('false'));
    ask('# second');

    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.context?.listing).toBeUndefined();
    expect(sentAt(1)?.context?.selected).toEqual(['a.txt']);
  });

  it('includes the other pane only after its chip is switched on, and explains chips in a tooltip', async () => {
    await mount();
    ask('# first');
    await vi.waitFor(() => expect(button('Other pane')).toBeDefined());
    expect(button('Other pane')?.getAttribute('aria-pressed')).toBe('false');
    button('2 names')?.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-tooltip')?.textContent).toContain('a.txt, src/'),
    );
    expect(root.querySelector('.fm-terminal-tooltip')?.textContent).toContain('Click to exclude');
    button('2 names')?.dispatchEvent(new MouseEvent('mouseleave'));
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-tooltip')).toBeNull());

    button('Other pane')?.click();
    await vi.waitFor(() => expect(button('Other pane')?.getAttribute('aria-pressed')).toBe('true'));
    ask('# second');

    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.context?.otherFolder).toBe('file:///other');
  });

  it('writes a markdown answer into the terminal as formatted text, not raw markup', async () => {
    assist.mockResolvedValueOnce({
      text: 'You have:\n- **keys** and `a.pem`\n- *old* ones',
    });
    await mount();
    ask('# what');

    await vi.waitFor(() => expect(root.textContent).toContain('• keys and a.pem'));
    expect(root.textContent).toContain('• old ones');
    expect(root.textContent).not.toContain('**');
    expect(root.textContent).not.toContain('`');
  });

  it('takes the typed keys back to the shell only once the answer is shown', async () => {
    let answer: ((reply: { text: string }) => void) | undefined;
    assist.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    await mount();
    ask('# slow');
    await vi.waitFor(() => expect(answer).toBeDefined());

    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => 'ls' } });
    textarea().dispatchEvent(paste);
    const sentNow = () => write.mock.calls.map(([, data]) => new TextDecoder().decode(data));
    expect(sentNow()).not.toContain('ls');

    answer?.({ text: 'Done.' });
    await vi.waitFor(() => expect(sentNow()).toContain('ls'));
    expect(sentNow().indexOf('\r')).toBeLessThan(sentNow().indexOf('ls'));
  });

  it('leaves a large folder listing out by default and sends it once its pill is switched on', async () => {
    paneListing = Array.from({ length: 65 }, (_, index) => `f${index}`);
    await mount();
    ask('# first');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)?.context?.listing).toBeUndefined();
    await vi.waitFor(() => expect(button('65 names')).toBeDefined());
    expect(button('65 names')?.getAttribute('aria-pressed')).toBe('false');

    button('65 names')?.click();
    await vi.waitFor(() => expect(button('65 names')?.getAttribute('aria-pressed')).toBe('true'));
    ask('# second');

    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.context?.listing).toHaveLength(65);
  });

  it('holds keys typed before the shell is ready, so they are not echoed twice', async () => {
    await mount();
    const paste = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => 'ls' } });
    textarea().dispatchEvent(paste);
    const sentNow = () => write.mock.calls.map(([, data]) => new TextDecoder().decode(data));
    expect(sentNow()).toEqual([]);

    echo?.({ type: 'output', data: bytes('\x1b]133;A\x07user % ') });
    await vi.waitFor(() => expect(sentNow()).toEqual(['ls']));
  });

  it('writes streamed lines and steps as they arrive, without writing the answer twice', async () => {
    assist.mockImplementationOnce(async (_request, onEvent) => {
      onEvent?.({ kind: 'step', text: 'Read a.txt' });
      onEvent?.({ kind: 'line', text: 'Streamed **line**' });
      onEvent?.({ kind: 'line', text: 'Command: ls' });
      return { text: 'Streamed **line**\nCommand: ls', steps: ['Read a.txt'] };
    });
    await mount();
    ask('# stream');

    await vi.waitFor(() => expect(root.textContent).toContain('Streamed line'));
    const text = root.textContent ?? '';
    expect(text.split('Streamed line')).toHaveLength(2);
    expect(text.split('Read a.txt')).toHaveLength(2);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls'),
    );
  });

  it('streams an approved edit continuation below the visible shell prompt', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      editRequest: { path: 'summaries.md', diff: '+Table', added: 1, removed: 0 },
    });
    assist.mockImplementationOnce(async (_request, onEvent) => {
      onEvent?.({ kind: 'line', text: 'Expanding the table.' });
      onEvent?.({ kind: 'step', text: 'Edited summaries.md' });
      return { text: 'Expanding the table.', steps: ['Edited summaries.md'] };
    });
    await mount();
    await markPrompt();
    ask('# expand the table');
    await vi.waitFor(() => expect(button('Allow')).toBeDefined());
    await markPrompt();
    button('Allow')?.click();

    const rows = () =>
      [...root.querySelectorAll('.xterm-rows > div')].map((row) => row.textContent?.trim());
    await vi.waitFor(() => {
      expect(rows()).toContain('user %');
      expect(rows()).toContain('Expanding the table.');
      expect(rows()).toContain('↳ Edited summaries.md');
    });
  });

  it('moves to the current chat line edges without sending Home or End to the shell', async () => {
    await mount();
    typeLine('#');
    const input = keyTarget(root, textarea());
    typeInChat(input, 'first\nsecond\nthird');
    input.setSelectionRange(9, 9);
    key('Home', 36);
    expect(input.selectionStart).toBe(6);
    key('End', 35);
    expect(input.selectionStart).toBe(12);
    expect(input.value).toBe('first\nsecond\nthird');
    expect(write).not.toHaveBeenCalled();
  });

  it('shows what a long tool call reports in the status line until its step arrives', async () => {
    let finish: (() => void) | undefined;
    assist.mockImplementationOnce(
      (_request, onEvent) =>
        new Promise((resolve) => {
          onEvent?.({ kind: 'progress', text: 'Summarising a.pdf: part 2 of 5' });
          finish = () => {
            onEvent?.({ kind: 'step', text: 'Summarised a.pdf (5 parts)' });
            resolve({ text: '', steps: ['Summarised a.pdf (5 parts)'] });
          };
        }),
    );
    await mount();
    ask('# summarise');

    const status = () => root.querySelector('.fm-terminal-assist-thinking')?.textContent ?? '';
    await vi.waitFor(() => expect(status()).toContain('Summarising a.pdf: part 2 of 5'));
    // The report is not written into the terminal as part of the answer.
    expect((root.textContent ?? '').split('part 2 of 5')).toHaveLength(2);

    finish?.();
    await vi.waitFor(() => expect(root.textContent).toContain('Summarised a.pdf (5 parts)'));
    expect(root.textContent).not.toContain('part 2 of 5');
  });

  it('folds the pills into one button that opens them', async () => {
    await mount();
    ask('# first');
    await vi.waitFor(() => expect(button('Context (3)')).toBeDefined());
    const toggle = button('Context (3)');
    expect(toggle?.getAttribute('aria-expanded')).toBe('false');
    expect(root.querySelector('.fm-terminal-context--open')).toBeNull();

    toggle?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-context--open')).not.toBeNull());
    expect(button('Context (3)')?.getAttribute('aria-expanded')).toBe('true');

    button('Context (3)')?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-context--open')).toBeNull());
  });

  it('leaves # to the shell and shows no assistant when the setting is off', async () => {
    assistantEnabled = false;
    await mount();
    ask('# not a question');

    await vi.waitFor(() => expect(write.mock.calls.map(([, data]) => data[0])).toContain(13));
    expect(assist).not.toHaveBeenCalled();
    expect(root.querySelector('.fm-terminal-assist-bar')).toBeNull();
    expect(root.querySelector('.fm-terminal-help')).toBeNull();
  });

  it('inserts the suggested command with Enter after a second question', async () => {
    await mount();
    ask('# first');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    ask('# second');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-command')?.textContent).toContain('ls'),
    );

    key('Enter', 13);

    await vi.waitFor(() =>
      expect(write).toHaveBeenLastCalledWith('session-1', new TextEncoder().encode('ls')),
    );
  });

  it('runs an approved command and sends its exit status and output back', async () => {
    assist.mockResolvedValueOnce({ text: '', runCommand: 'cargo test' });
    await mount();
    await markPrompt();
    ask('# run the tests');

    await vi.waitFor(() => expect(button('Run')).toBeDefined());
    expect(write).not.toHaveBeenCalledWith(
      'session-1',
      new TextEncoder().encode('\x15 cargo test\r'),
    );
    button('Run')?.click();
    await vi.waitFor(() =>
      expect(write).toHaveBeenCalledWith(
        'session-1',
        new TextEncoder().encode('\x15 cargo test\r'),
      ),
    );

    echo?.({ type: 'output', data: bytes('\x1b]133;C\x07\r\n1 failed\r\n\x1b]133;D;3\x07') });
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.toolResult).toMatchObject({ command: 'cargo test', exitCode: 3 });
    expect(sentAt(1)?.toolResult?.output).toContain('1 failed');
  });

  it('runs a read-only command at once, without a Run button', async () => {
    assist.mockResolvedValueOnce({ text: '', runCommand: 'grep -n foo src | head -5' });
    await mount();
    await markPrompt();
    ask('# find foo');

    await vi.waitFor(() =>
      expect(write).toHaveBeenCalledWith(
        'session-1',
        new TextEncoder().encode('\x15 grep -n foo src | head -5\r'),
      ),
    );
    expect(button('Run')).toBeUndefined();

    echo?.({ type: 'output', data: bytes('\x1b]133;C\x07\r\nsrc:1:foo\r\n\x1b]133;D;0\x07') });
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.toolResult).toMatchObject({ command: 'grep -n foo src | head -5' });
  });

  it('runs a python script that only reads at once, as it does ls', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      runCommand: 'python3 count.py',
      runDetails: {
        explanation: 'Counts the JSON files.',
        scripts: [
          {
            path: 'count.py',
            text: 'from pathlib import Path\nprint(len(list(Path(".").glob("*.json"))))',
            truncated: false,
          },
        ],
      },
    });
    await mount();
    await markPrompt();
    ask('# how many json files?');
    await vi.waitFor(() =>
      expect(write).toHaveBeenCalledWith(
        'session-1',
        new TextEncoder().encode('\x15 python3 count.py\r'),
      ),
    );
    expect(button('Run')).toBeUndefined();
  });

  it('asks for a script that deletes, saying why and what Procyon found', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      runCommand: 'python3 clean.py',
      runDetails: {
        explanation: 'Removes the cache folders.',
        scripts: [
          { path: 'clean.py', text: "import shutil\nshutil.rmtree('cache')", truncated: false },
        ],
      },
    });
    let answer: (text: string) => void = () => undefined;
    const assistDescribe = vi.fn(
      (_command: string, _folder: string) =>
        new Promise<string | null>((resolve) => {
          answer = resolve;
        }),
    );
    await mountWith({ assistDescribe });
    await markPrompt();
    ask('# clean the caches');
    await vi.waitFor(() => expect(button('Run')).toBeDefined());
    let text = root.textContent ?? '';
    expect(text).toContain("The assistant's reason: Removes the cache folders.");
    expect(text).toContain('clean.py: deletes files');
    // The description comes from a call that is given only the command and its folder.
    expect(assistDescribe).toHaveBeenCalledWith('python3 clean.py', 'file:///home/repo');
    expect(text).toContain('Asking a separate model call what the command does');
    answer('It deletes the cache folder.');
    await vi.waitFor(() => {
      text = root.textContent ?? '';
      expect(text).toContain(
        'What it does, read by a separate model call that saw only the command: It deletes the cache folder.',
      );
    });
    // The command itself is folded below what it does.
    expect(
      root.querySelector('details.fm-terminal-assist-folded .fm-terminal-command-show')
        ?.textContent,
    ).toBe('Show command');
    expect(write).not.toHaveBeenCalledWith(
      'session-1',
      new TextEncoder().encode('\x15 python3 clean.py\r'),
    );
  });

  it('describes a suggested command and keeps the summary visible when inspecting its full text', async () => {
    const command = `python3 -c "print('${'x'.repeat(240)}')"`;
    const assistDescribe = vi.fn(async () => 'Prints a long string.');
    assist.mockResolvedValueOnce({ text: `Command: ${command}` });
    await mountWith({ assistDescribe });
    await markPrompt();
    ask('# suggest a command');

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-description')?.textContent).toContain(
        'Prints a long string.',
      ),
    );
    expect(assistDescribe).toHaveBeenCalledWith(command, 'file:///home/repo');
    for (const label of ['Insert', 'Edit', 'Dismiss']) {
      expect(button(label)).toBeDefined();
    }
    const disclosure = root.querySelector<HTMLDetailsElement>('.fm-terminal-assist-folded');
    expect(disclosure?.open).toBe(false);
    expect(disclosure?.querySelector('pre')?.textContent).toBe(command);
    disclosure?.querySelector('summary')?.click();
    expect(disclosure?.open).toBe(true);
    expect(disclosure?.querySelector('.fm-terminal-command-hide')?.textContent).toBe(
      'Hide command',
    );
    expect(root.querySelector('.fm-terminal-assist-description')?.textContent).toContain(
      'Prints a long string.',
    );
    disclosure?.querySelector('summary')?.click();
    expect(disclosure?.open).toBe(false);
  });

  it('shows an unavailable summary when describing an approval fails', async () => {
    assist.mockResolvedValueOnce({ text: '', runCommand: 'python3 generate.py' });
    await mountWith({ assistDescribe: vi.fn().mockRejectedValue(new Error('timed out')) });
    await markPrompt();
    ask('# generate a report');
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-description')?.textContent).toBe(
        'Command summary unavailable.',
      ),
    );
    expect(button('Run')).toBeDefined();
  });

  it('in auto-approve mode runs a command at once, but still asks for a dangerous one', async () => {
    assist
      .mockResolvedValueOnce({ text: '', runCommand: 'rm -rf build' })
      .mockResolvedValueOnce({ text: '', runCommand: 'rm -rf ~' });
    await mount({ homeDirectory: '/home' });
    await markPrompt();
    ask('# /auto');
    await vi.waitFor(() => expect(root.textContent).toContain('Auto-approve is on'));
    await markPrompt();
    ask('# clean');

    await vi.waitFor(() =>
      expect(write).toHaveBeenCalledWith(
        'session-1',
        new TextEncoder().encode('\x15 rm -rf build\r'),
      ),
    );
    expect(sentAt(0)?.autoApprove).toBe(true);
    echo?.({ type: 'output', data: bytes('\x1b]133;C\x07\r\n\x1b]133;D;0\x07') });

    await vi.waitFor(() => expect(button('Run')).toBeDefined());
    expect(root.querySelector('.fm-terminal-assist-danger')?.textContent).toContain(
      'your home folder',
    );
    expect(write).not.toHaveBeenCalledWith(
      'session-1',
      new TextEncoder().encode('\x15 rm -rf ~\r'),
    );
  });

  it('still asks for a read-only command when the setting is off', async () => {
    autoRunReadOnly = false;
    assist.mockResolvedValueOnce({ text: '', runCommand: 'ls -la' });
    await mount();
    await markPrompt();
    ask('# list');

    await vi.waitFor(() => expect(button('Run')).toBeDefined());
    expect(write).not.toHaveBeenCalledWith('session-1', new TextEncoder().encode('\x15 ls -la\r'));
  });

  it('completes an @name with Tab from the folder, quoting a name with spaces', async () => {
    folderNames = ['ATP 7-100.1 Russian Tactics  2024.pdf', 'notes.md'];
    await mount();
    await markPrompt();
    typeLine('# read @ATP');

    key('Tab', 9);

    await vi.waitFor(() =>
      expect(chatInput(root)?.value).toBe('read @"ATP 7-100.1 Russian Tactics  2024.pdf" '),
    );
  });

  const chatText = () => chatInput(root)?.value;
  const clearChat = () => {
    const input = chatInput(root);
    if (input === null) throw new Error('chat input missing');
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };

  it('completes an @path anywhere under the session root and lists again after a change', async () => {
    let files = ['README.md', 'src/', 'src/deep/', 'src/deep/main.rs'];
    const assistFiles = vi.fn(async () => ({ files, truncated: false }));
    await mountWith({ assistFiles });
    typeLine('# read @main');
    key('Tab', 9);
    await vi.waitFor(() => expect(chatText()).toBe('read @src/deep/main.rs '));
    expect(assistFiles).toHaveBeenCalledWith('file:///home/repo');
    clearChat();

    // A command ran: the next completion lists the tree again and sees the new file.
    files = [...files, 'src/new.rs'];
    ask('# where is it?');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)?.root).toBe('file:///home/repo');
    await markPrompt();
    typeLine('# open @new');
    key('Tab', 9);
    await vi.waitFor(() => expect(chatText()).toBe('open @src/new.rs '));
    expect(assistFiles).toHaveBeenCalledTimes(2);
  });

  it('starts reading the paths when @ is typed, and Tab then uses that one read', async () => {
    let release: (() => void) | undefined;
    const assistFiles = vi.fn(
      () =>
        new Promise<{ files: readonly string[]; truncated: boolean }>((resolve) => {
          release = () => resolve({ files: ['src/deep/main.rs'], truncated: false });
        }),
    );
    await mountWith({ assistFiles });
    typeLine('# read @');
    await vi.waitFor(() => expect(assistFiles).toHaveBeenCalledOnce());
    typeLine('main');
    key('Tab', 9);
    release?.();
    await vi.waitFor(() => expect(chatText()).toBe('read @src/deep/main.rs '));
    expect(assistFiles).toHaveBeenCalledOnce();
  });

  async function mountWith(
    client: Partial<TerminalClient>,
    extra: Partial<TerminalDrawerAttrs> = {},
  ) {
    m.mount(root, {
      view: () =>
        m(TerminalDrawer, {
          ...extra,
          open: true,
          tabKey: 'pane-1:tab-1',
          location: { providerId: 'local', uri: 'file:///home/repo' },
          client: {
            open: vi.fn(async (_location, _cols, _rows, output) => {
              echo = output;
              return 'session-1';
            }),
            write,
            resize: vi.fn(async () => undefined),
            close: vi.fn(async () => true),
            assist,
            assistClose,
            ...client,
          },
          onToggle: vi.fn(),
          shellIntegration: 'disabled',
          assistantEnabled: true,
        }),
    });
    await vi.waitFor(() => expect(root.querySelector('.xterm-helper-textarea')).not.toBeNull());
    await vi.waitFor(() => expect(echo).toBeDefined());
    await markPrompt();
  }

  it('completes / with commands and skills, and asks with an invoked skill', async () => {
    const assistSkills = vi.fn(async () => [
      { name: 'release', description: 'Releases', source: 'project' as const },
      { name: 'nginx-cert', description: 'Certs', source: 'note' as const },
    ]);
    await mountWith({ assistSkills });
    ask('# /release v1.2');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)).toMatchObject({
      request: 'v1.2',
      skill: 'release',
      root: 'file:///home/repo',
    });

    await markPrompt();
    typeLine('# /rel');
    key('Tab', 9);
    await vi.waitFor(() => expect(chatText()).toBe('/release '));
    expect(assistSkills).toHaveBeenCalledWith('file:///home/repo');
  });

  it('runs /clear, /new and /compact without asking the model', async () => {
    const assistCompact = vi.fn(async () => undefined);
    await mountWith({ assistCompact });
    ask('# /compact');
    await vi.waitFor(() => expect(assistCompact).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(root.textContent).toContain('replaced by a summary'));
    await markPrompt();
    ask('# /clear');
    await vi.waitFor(() => expect(assistClose).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(root.textContent).toContain('Conversation cleared.'));
    await markPrompt();
    ask('# /new');
    await vi.waitFor(() => expect(assistClose).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(root.textContent).toContain('New session in file:///home/repo'));
    expect(assist).not.toHaveBeenCalled();
  });

  it('lists saved sessions with /resume and continues one with /resume <number>', async () => {
    const saved = {
      id: 'old-tab',
      title: 'where are the decoy files?',
      folder: 'file:///home/repo',
      updatedAt: '2026-11-24T10:00:00Z',
      questions: 3,
    };
    const assistSaved = vi.fn(async () => [saved]);
    const assistResume = vi.fn(async (_savedId: string, _sessionId: string) => saved);
    await mountWith({ assistSaved, assistResume });
    ask('# /resume');
    await vi.waitFor(() => expect(root.textContent).toContain('1. where are the decoy files?'));
    await markPrompt();
    ask('# /resume 1');
    await vi.waitFor(() => expect(assistResume).toHaveBeenCalledOnce());
    expect(assistResume.mock.calls[0]?.[0]).toBe('old-tab');
    await vi.waitFor(() => expect(root.textContent).toContain('Continuing'));
    expect(assist).not.toHaveBeenCalled();
  });

  it('starts /resume and /new under a new session id, so saved sessions are not overwritten', async () => {
    const saved = {
      id: 'old-tab',
      title: 'decoys',
      folder: 'file:///home/repo',
      updatedAt: '2026-11-24T10:00:00Z',
      questions: 1,
    };
    const assistResume = vi.fn(async (_savedId: string, _sessionId: string) => saved);
    await mountWith({ assistSaved: async () => [saved], assistResume });
    ask('# /clear');
    await vi.waitFor(() => expect(assistClose).toHaveBeenCalledOnce());
    const first = assistClose.mock.calls[0]?.[0];
    await markPrompt();
    ask('# /resume');
    await vi.waitFor(() => expect(root.textContent).toContain('1. decoys'));
    await markPrompt();
    ask('# /resume 1');
    await vi.waitFor(() => expect(assistResume).toHaveBeenCalledOnce());
    const into = assistResume.mock.calls[0]?.[1];
    expect(into).toBeDefined();
    expect(into).not.toBe(first);
    expect(into).not.toBe('old-tab');
  });

  it('deletes a listed saved session with /resume delete <number>', async () => {
    const saved = {
      id: 'old-tab',
      title: 'decoys',
      folder: 'file:///home/repo',
      updatedAt: '2026-11-24T10:00:00Z',
      questions: 1,
    };
    const assistDeleteSaved = vi.fn(async (_savedId: string) => true);
    await mountWith({
      assistSaved: async () => [saved],
      assistResume: async () => saved,
      assistDeleteSaved,
    });
    ask('# /resume');
    await vi.waitFor(() => expect(root.textContent).toContain('1. decoys'));
    await markPrompt();
    ask('# /resume delete 1');
    await vi.waitFor(() => expect(assistDeleteSaved).toHaveBeenCalledWith('old-tab'));
    await vi.waitFor(() => expect(root.textContent).toContain('Deleted “decoys”.'));
    expect(assist).not.toHaveBeenCalled();
  });

  it('lists questions with /fork and forks before one, with it in the input', async () => {
    const assistQuestions = vi.fn(async (_sessionId: string) => ['first?', 'second?']);
    const assistFork = vi.fn(async (_sessionId: string, question: number, _into: string) =>
      question === 2 ? 'second?' : null,
    );
    await mountWith({ assistQuestions, assistFork });
    ask('# /fork');
    await vi.waitFor(() => expect(root.textContent).toContain('2. second?'));
    await markPrompt();
    ask('# /fork 2');
    await vi.waitFor(() => expect(assistFork).toHaveBeenCalledOnce());
    const [from, question, into] = assistFork.mock.calls[0] ?? [];
    expect(question).toBe(2);
    expect(into).not.toBe(from);
    await vi.waitFor(() =>
      expect(root.textContent).toContain('New session from before question 2'),
    );
    await vi.waitFor(() => expect(chatText()).toBe('second?'));
    expect(assist).not.toHaveBeenCalled();
  });

  it('lets the assistant read web pages unless /web off turns it off for the session', async () => {
    await mountWith({});
    ask('# what changed upstream?');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)).toMatchObject({ allowWeb: true });
    await markPrompt();
    ask('# /web off');
    await vi.waitFor(() =>
      expect(root.textContent).toContain('Reading web pages is off for this session.'),
    );
    await markPrompt();
    ask('# and now?');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)).toMatchObject({ allowWeb: false });
    await markPrompt();
    ask('# /web on');
    await vi.waitFor(() =>
      expect(root.textContent).toContain(
        'The assistant may read public web pages in this session.',
      ),
    );
  });

  it('sets the session thinking effort with /effort and sends it with each question', async () => {
    await mountWith({});
    ask('# /effort high');
    await vi.waitFor(() =>
      expect(root.textContent).toContain('Thinking effort for this session: High.'),
    );
    await markPrompt();
    ask('# /effort sometimes');
    await vi.waitFor(() => expect(root.textContent).toContain('Use /effort off, low'));
    await markPrompt();
    ask('# what is here?');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)).toMatchObject({ thinking: 'high' });
    await markPrompt();
    ask('# /effort default');
    await vi.waitFor(() => expect(root.textContent).toContain("the chat model's default"));
  });

  it('lists the chat models with /model and switches with /model <number>', async () => {
    const select = vi.fn<TerminalModels['select']>(async () => undefined);
    const models: TerminalModels = {
      list: async () => ({
        profiles: [
          { id: 'p1', name: 'Local', model: 'qwen' } as unknown as LlmProfile,
          { id: 'p2', name: 'Cloud', model: 'big' } as unknown as LlmProfile,
        ],
        chat: { profileId: 'p1', model: 'qwen', allowCloud: false },
      }),
      discover: async (id) => (id === 'p1' ? ['qwen', 'llama'] : []),
      select,
    };
    await mountWith({}, { models });
    ask('# /model');
    await vi.waitFor(() => expect(root.textContent).toContain('2. llama (Local)'));
    expect(root.textContent).toContain('* 1. qwen (Local)');
    await markPrompt();
    ask('# /model 3');
    await vi.waitFor(() => expect(select).toHaveBeenCalledOnce());
    expect(select.mock.calls[0]?.[0]).toMatchObject({ profileId: 'p2', model: 'big' });
    await vi.waitFor(() => expect(root.textContent).toContain('Chat model: big (Cloud).'));
  });

  it('saves the session as a note with Ctrl+Shift+S', async () => {
    saveNote.mockClear();
    await mount();
    await markPrompt();
    ask('# hello');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());

    textarea().dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'S',
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(saveNote).toHaveBeenCalledOnce();
  });

  it('opens the chat input from Ask, and Esc or Backspace on an empty input go back to the shell', async () => {
    await mount();
    await markPrompt();
    button('Ask')?.click();
    await vi.waitFor(() => expect(document.activeElement).toBe(chatInput(root)));
    expect(button('Ask')?.getAttribute('aria-pressed')).toBe('true');

    typeLine('half a question');
    key('Escape', 27);
    expect(chatInput(root)).toBeNull();
    expect(document.activeElement).toBe(textarea());

    typeLine('#');
    await vi.waitFor(() => expect(chatInput(root)?.value).toBe(''));
    key('Backspace', 8);
    expect(chatInput(root)).toBeNull();
    expect(assist).not.toHaveBeenCalled();
  });

  it('shows the folder on the status line in the shell, the pills in the chat, and sends with the arrow', async () => {
    await mount();
    await markPrompt();
    m.redraw.sync();
    expect(root.querySelector('.fm-terminal-status-folder')?.textContent).toBe('/home');
    expect(root.querySelector('.fm-terminal-context')).toBeNull();

    typeLine('# what is here?');
    expect(root.querySelector('.fm-terminal-status-folder')).toBeNull();
    expect(root.querySelector('.fm-terminal-context')).not.toBeNull();
    root.querySelector<HTMLButtonElement>('.fm-terminal-composer-send')?.click();

    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)?.request).toBe('what is here?');
    m.redraw.sync();
    expect(root.querySelector<HTMLButtonElement>('.fm-terminal-composer-send')?.disabled).toBe(
      true,
    );
  });

  it('asks a question written while an answer is coming once that answer is done', async () => {
    let answer: ((reply: { text: string }) => void) | undefined;
    assist.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    await mount();
    await markPrompt();
    ask('# slow');
    await vi.waitFor(() => expect(answer).toBeDefined());
    ask('# next');
    expect(chatInput(root)?.value).toBe('');
    expect(root.textContent).toContain('Sends after this answer');
    expect(assist).toHaveBeenCalledOnce();

    answer?.({ text: 'Done.' });
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.request).toBe('next');
  });

  it('sends a message written while the assistant works to the running request', async () => {
    let answer: ((reply: { text: string }) => void) | undefined;
    let events: ((event: TerminalAssistEvent) => void) | undefined;
    assist.mockImplementationOnce(
      (_request, onEvent) =>
        new Promise((resolve) => {
          answer = resolve;
          events = onEvent;
        }),
    );
    const assistSteer = vi.fn(async () => true);
    await mountWith({ assistSteer });
    await markPrompt();
    ask('# summarise the folder');
    await vi.waitFor(() => expect(answer).toBeDefined());
    ask('# skip the PDFs');
    ask('# and the images');

    expect(assistSteer).toHaveBeenCalledWith(expect.any(String), 'skip the PDFs');
    expect(root.textContent).toContain('reads it after its current step');
    events?.({ kind: 'steered', text: 'skip the PDFs' });

    answer?.({ text: 'Done.' });
    // The model never read the second one, so it is asked next.
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.request).toBe('and the images');
  });

  it('shows a message sent while the assistant works until it is read, and takes it back', async () => {
    let answer: ((reply: { text: string }) => void) | undefined;
    assist.mockImplementationOnce(
      (_request, onEvent) =>
        new Promise((resolve) => {
          answer = resolve;
          void onEvent;
        }),
    );
    const assistSteer = vi.fn(async () => true);
    const assistWithdraw = vi.fn(async () => true);
    await mountWith({ assistSteer, assistWithdraw });
    await markPrompt();
    ask('# summarise the folder');
    await vi.waitFor(() => expect(answer).toBeDefined());
    ask('# skip the PDFs');

    // Listed at once above the input, not only as a hint.
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-pending')?.textContent).toContain('skip the PDFs'),
    );
    button('Take back')?.click();
    await vi.waitFor(() =>
      expect(assistWithdraw).toHaveBeenCalledWith(expect.any(String), 'skip the PDFs'),
    );
    // Back in the input to edit; no longer listed as sent.
    await vi.waitFor(() => expect(chatInput(root)?.value).toBe('skip the PDFs'));
    expect(root.querySelector('.fm-terminal-pending')).toBeNull();
    answer?.({ text: 'Done.' });
    // Taken back, so it is not asked again as a question.
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(assist).toHaveBeenCalledOnce();
  });

  it('recalls earlier questions with Up and Down in the chat input', async () => {
    await mount();
    await markPrompt();
    ask('# first');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(1));
    ask('# second');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    typeLine('draft');

    key('ArrowUp', 38);
    expect(chatInput(root)?.value).toBe('second');
    key('ArrowUp', 38);
    expect(chatInput(root)?.value).toBe('first');
    key('ArrowDown', 40);
    expect(chatInput(root)?.value).toBe('second');
    key('ArrowDown', 40);
    expect(chatInput(root)?.value).toBe('draft');
    // The shell never saw any of it.
    const typed = write.mock.calls.map(([, data]) => new TextDecoder().decode(data)).join('');
    expect(typed).not.toContain('first');
  });

  it('leaves Up to the shell on a line that is not a question', async () => {
    await mount();
    await markPrompt();
    ask('# first');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(1));
    // Back in the terminal, Up is the shell's own history.
    textarea().focus();
    const before = write.mock.calls.length;
    key('ArrowUp', 38);
    await vi.waitFor(() =>
      expect(
        write.mock.calls.slice(before).map(([, data]) => new TextDecoder().decode(data)),
      ).toContain('\x1b[A'),
    );
  });

  it('uses the shell working directory for chat even when the pane stays put', async () => {
    await mount();
    await markPrompt();
    echo?.({ type: 'output', data: bytes('\x1b]133;P;Cwd=/tmp/source files\x07') });
    await markPrompt();
    ask('# Write the report to summaries.md');
    await vi.waitFor(() =>
      expect(sentAt(0)).toMatchObject({
        workingDirectory: 'file:///tmp/source%20files',
        root: 'file:///tmp/source%20files',
      }),
    );
    await markPrompt();
    echo?.({ type: 'output', data: bytes('\x1b]133;P;Cwd=/tmp/source files/archive\x07') });
    await markPrompt();
    ask('# Edit it');
    await vi.waitFor(() =>
      expect(sentAt(1)).toMatchObject({
        workingDirectory: 'file:///tmp/source%20files/archive',
        root: 'file:///tmp/source%20files',
      }),
    );
  });

  it('shows shell-reported commands in the context chip after history recall', async () => {
    await mount();
    for (const command of ['pwd', 'ls -la']) {
      await markPrompt();
      key('ArrowUp', 38);
      key('Enter', 13);
      echo?.({
        type: 'output',
        data: bytes(`\x1b]133;C;Command=${command}\x07\r\noutput\r\n\x1b]133;D;0\x07`),
      });
    }

    await markPrompt();
    ask('# what ran?');
    await vi.waitFor(() =>
      expect(button('Last 2 commands')?.getAttribute('aria-label')).toContain('pwd, ls -la'),
    );
    expect(sentAt(0)?.context?.blocks?.map((block) => block.command)).toEqual(['pwd', 'ls -la']);
  });

  it('offers Explain after a command and Fix only after a failed one', async () => {
    await mount();
    await markPrompt();
    ask('# hello');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(button('Explain')).toBeUndefined();
    expect(button('Suggest a fix')).toBeUndefined();

    ask('ls');
    echo?.({ type: 'output', data: bytes('\x1b]133;C\x07\r\nok\r\n\x1b]133;D;0\x07') });
    await vi.waitFor(() => expect(button('Explain')).toBeDefined());
    expect(button('Suggest a fix')).toBeUndefined();

    ask('false');
    echo?.({
      type: 'output',
      data: bytes('\x1b]133;A\x07\x1b]133;C\x07\r\nbad\r\n\x1b]133;D;2\x07'),
    });
    await vi.waitFor(() => expect(button('Suggest a fix')).toBeDefined());
  });

  it('asks once before creating files and sends the choice back', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      writeRequest: { folder: './', name: 'a.md', bytes: 1300 },
    });
    await mount();
    await markPrompt();
    ask('# summarise the documents');

    await vi.waitFor(() => expect(root.textContent).toContain('Create files in ./?'));
    expect(root.textContent).toContain('a.md (1.3 KiB)');
    expect(sentAt(0)?.allowFileWrites).toBe(true);

    button('Allow')?.click();
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.writeDecision).toBe('question');
    expect(sentAt(1)?.sessionId).toBe(sentAt(0)?.sessionId);
  });

  it('can allow file creation for the session', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      writeRequest: { folder: './', name: 'a.md', bytes: 10 },
    });
    await mount();
    await markPrompt();
    ask('# summarise');

    await vi.waitFor(() => expect(button('This session')).toBeDefined());
    button('This session')?.click();
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.writeDecision).toBe('session');
  });

  it('declining file creation tells the session and shows nothing more', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      writeRequest: { folder: './', name: 'a.md', bytes: 10 },
    });
    await mount();
    await markPrompt();
    ask('# summarise');

    await vi.waitFor(() => expect(button('Decline')).toBeDefined());
    button('Decline')?.click();
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.writeDecision).toBe('decline');
    await vi.waitFor(() => expect(button('Decline')).toBeUndefined());
  });

  it('shows a proposed edit as a diff and sends the answer back', async () => {
    assist.mockResolvedValueOnce({
      text: '',
      editRequest: {
        path: 'notes.md',
        diff: '--- notes.md\n+++ notes.md\n@@ -1 +1 @@\n-Old line.\n+New line.\n',
        added: 1,
        removed: 1,
      },
    });
    await mount();
    ask('# fix the note');

    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-diff')).not.toBeNull());
    expect(root.textContent).toContain('Change notes.md?');
    expect(
      root.querySelector('.fm-terminal-assist-diff .fm-terminal-diff-added')?.textContent,
    ).toBe('+New line.\n');
    button('This session')?.click();
    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.editDecision).toBe('session');
  });

  it('tells the host not to offer file creation when the setting is off', async () => {
    await mount({ allowFileWrites: false });
    ask('# summarise');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)?.allowFileWrites).toBe(false);
  });

  it('tells the session when a command is declined and shows nothing more', async () => {
    assist.mockResolvedValueOnce({ text: '', runCommand: 'rm -rf build' });
    await mount();
    await markPrompt();
    ask('# clean');

    await vi.waitFor(() => expect(button('Decline')).toBeDefined());
    button('Decline')?.click();

    await vi.waitFor(() => expect(assist).toHaveBeenCalledTimes(2));
    expect(sentAt(1)?.toolResult).toEqual({ command: 'rm -rf build', declined: true });
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-command')).toBeNull());
  });

  it('opens a help dialog from the question mark and closes it with Escape', async () => {
    await mount();
    expect(root.querySelector('.fm-terminal-help-dialog')).toBeNull();

    root.querySelector<HTMLButtonElement>('.fm-terminal-help')?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-help-dialog')).not.toBeNull());
    const dialog = root.querySelector('.fm-terminal-help-dialog');
    expect(dialog?.getAttribute('role')).toBe('dialog');
    expect(dialog?.getAttribute('aria-modal')).toBe('true');
    expect(dialog?.textContent).toContain('@file');
    expect(dialog?.textContent).not.toContain('@dir');
    expect(dialog?.textContent).toContain('Ctrl+Shift+E');

    dialog?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-help-dialog')).toBeNull());

    root.querySelector<HTMLButtonElement>('.fm-terminal-help')?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-help-dialog')).not.toBeNull());
    root
      .querySelector<HTMLButtonElement>('.fm-terminal-help-dialog .fm-terminal-help-close')
      ?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-help-dialog')).toBeNull());
  });

  it('forgets the conversation when the tab closes', async () => {
    await mount();
    ask('# hello');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());

    disposeTab?.('pane-1:tab-1');

    expect(assistClose).toHaveBeenCalledWith(sentAt(0)?.sessionId);
  });

  it('explains from the visible output when the shell reports no command blocks', async () => {
    await mount();
    echo?.({ type: 'output', data: bytes('error: boom\r\n') });
    await vi.waitFor(() => expect(root.textContent).toContain('boom'));

    const shortcut = new KeyboardEvent('keydown', {
      key: 'e',
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(shortcut, 'keyCode', { value: 69 });
    textarea().dispatchEvent(shortcut);

    await vi.waitFor(() =>
      expect(assist).toHaveBeenCalledWith(
        expect.objectContaining({ kind: 'ask' }),
        expect.any(Function),
      ),
    );
    expect(assist.mock.calls.at(-1)?.[0].context?.blocks?.[0]?.output).toContain('boom');
  });

  it('saves the session as a note with its conversation and output (task 0337)', async () => {
    const onSaveAsNote = vi.fn();
    await mount({ onSaveAsNote, workspaceId: 'ws-1' });
    echo?.({ type: 'output', data: bytes('certbot renew done\r\n') });
    await vi.waitFor(() => expect(root.textContent).toContain('certbot renew done'));
    ask('# how do I renew?');
    await vi.waitFor(() => expect(assist).toHaveBeenCalledOnce());
    expect(sentAt(0)?.workspaceId).toBe('ws-1');
    await vi.waitFor(() => expect(button('Save as note')).toBeDefined());

    button('Save as note')?.click();

    expect(onSaveAsNote).toHaveBeenCalledWith({
      sessionId: sentAt(0)?.sessionId,
      blocks: [expect.objectContaining({ output: expect.stringContaining('certbot renew') })],
      workingDirectory: 'file:///home',
    });
  });

  it('hands a note the assistant proposed to the owner for review (task 0337)', async () => {
    const onNoteProposal = vi.fn();
    const proposal = {
      draft: { title: 'T', fileName: 't.md', tags: [], content: '# T', redactions: 0 },
    };
    assist.mockResolvedValueOnce({ text: '', noteProposal: proposal });
    await mount({ onNoteProposal });
    ask('# write this down as a note');

    await vi.waitFor(() => expect(onNoteProposal).toHaveBeenCalledWith(proposal));
    await vi.waitFor(() => expect(root.textContent).toContain('proposed a note'));
  });
});
