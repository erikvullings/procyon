import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ActionDescriptor } from '../../models';
import { CommandPalette } from './command-palette';

const action = (id: string, title: string): ActionDescriptor => ({
  id,
  title,
  category: 'fileOperations',
  defaultShortcuts: [],
  contextRequirements: {},
  source: { kind: 'core' },
});

const actions = [
  action('core.copyPath', 'Copy Path'),
  action('core.createDirectory', 'New Folder'),
];

afterEach(() => {
  document.body.innerHTML = '';
});

function mountOpen(onInvoke: (a: ActionDescriptor) => void): void {
  const root = document.createElement('div');
  document.body.append(root);
  m.mount(root, {
    view: () =>
      m(CommandPalette, {
        open: true,
        actions,
        recency: new Map(),
        context: {} as never,
        availabilityContext: {
          selectedEntries: [],
          locationWritable: true,
          clipboardHasEntries: false,
          openTerminalSupported: false,
        },
        onClose: () => {},
        onInvoke,
      }),
  });
  m.redraw.sync();
}

function press(input: HTMLInputElement, key: string): void {
  input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  m.redraw.sync();
}

function type(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  m.redraw.sync();
}

describe('command palette keyboard', () => {
  it('runs the top match on Enter after typing', async () => {
    const onInvoke = vi.fn();
    mountOpen(onInvoke);
    const input = await vi.waitFor(() => {
      const found = document.querySelector<HTMLInputElement>('.mm-command-palette input');
      if (found === null) throw new Error('no input');
      return found;
    });
    type(input, 'new');
    press(input, 'Enter');
    expect(onInvoke).toHaveBeenCalledWith(expect.objectContaining({ id: 'core.createDirectory' }));
  });

  it('moves with arrow keys', async () => {
    const onInvoke = vi.fn();
    mountOpen(onInvoke);
    const input = document.querySelector<HTMLInputElement>('.mm-command-palette input');
    if (input === null) throw new Error('no input');
    press(input, 'ArrowDown');
    press(input, 'ArrowDown');
    press(input, 'Enter');
    expect(onInvoke).toHaveBeenCalledOnce();
  });
});
