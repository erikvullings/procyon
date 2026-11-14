import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TerminalClient } from './terminal-client';
import {
  handleTerminalKeyEvent,
  questionAtPrompt,
  showTerminalSurface,
  TerminalDrawer,
  terminalHost,
} from './terminal-drawer';

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
    textarea.dispatchEvent(event);
  }

  it('sends a # question to the assistant and clears the line instead of running it', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const assist = vi.fn(async () => ({ text: 'ls -la' }));
    mount({
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist,
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');

    pasteText(textarea, '#list files');
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    pressEnter(textarea);

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-text')?.textContent).toBe('ls -la'),
    );
    expect(assist).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: 'command',
        request: 'list files',
        workingDirectory: 'file:///home',
      }),
    );
    const sent = write.mock.calls.map(([, data]) => new TextDecoder().decode(data));
    expect(sent).toContain('\x15');
    expect(sent).not.toContain('\r');
    expect(sent).not.toContain('\n');
  });

  it('inserts an answer at the prompt without running it', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    mount({
      open: vi.fn(async () => 'session-1'),
      write,
      resize: vi.fn(async () => undefined),
      close: vi.fn(async () => true),
      assist: vi.fn(async () => ({ text: 'ls -la' })),
    });
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')).not.toBeNull(),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    const textarea = root.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea');
    if (textarea === null) throw new Error('xterm textarea missing');
    pasteText(textarea, '#list files');
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    pressEnter(textarea);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-text')?.textContent).toBe('ls -la'),
    );

    const insert = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.startsWith('Insert at prompt') === true,
    );
    insert?.click();

    await vi.waitFor(() =>
      expect(write).toHaveBeenLastCalledWith('session-1', new TextEncoder().encode('ls -la')),
    );
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-panel')).toBeNull());
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
      assist: vi.fn(async () => ({ text: 'ls -la' })),
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
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    Object.defineProperty(enter, 'keyCode', { value: 13 });
    textarea.dispatchEvent(enter);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-text')?.textContent).toBe('ls -la'),
    );
    const insert = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.startsWith('Insert at prompt') === true,
    );
    insert?.click();
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(3));

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

  it('still asks when the line was edited with an arrow key first', async () => {
    const write = vi.fn<TerminalClient['write']>(async () => undefined);
    const assist = vi.fn(async () => ({ text: 'ls -la' }));
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
      textarea.dispatchEvent(event);
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
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(2));
    key('Enter', 13);

    await vi.waitFor(() =>
      expect(assist).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: 'command',
          request: 'How to list all files sorted by size',
        }),
      ),
    );
    const sent = write.mock.calls.map(([, data]) => [...data]);
    expect(sent.some(([byte]) => byte === 13)).toBe(false);
  });
});

describe('questionAtPrompt', () => {
  it('reads a question from the tracked input when it is known', () => {
    expect(questionAtPrompt({ text: '# list files', known: true }, '')).toBe('list files');
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
      assist: vi.fn(async () => ({ text: 'ls -la' })),
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
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    press(textarea(), 'Enter', {}, 13);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-terminal-assist-text')?.textContent).toBe('ls -la'),
    );
  }

  it('inserts a shown answer with Enter, without sending Enter to the shell', async () => {
    await askForAnswer();
    press(textarea(), 'Enter', {}, 13);

    await vi.waitFor(() => expect(sent()).toContainEqual([...new TextEncoder().encode('ls -la')]));
    expect(sent().some(([byte]) => byte === 13)).toBe(false);
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-panel')).toBeNull());
  });

  it('rejects a shown answer with Ctrl+C, and sends Ctrl+C to the shell when no answer is shown', async () => {
    await askForAnswer();
    press(textarea(), 'c', { ctrlKey: true }, 67);
    await vi.waitFor(() => expect(root.querySelector('.fm-terminal-assist-panel')).toBeNull());
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
