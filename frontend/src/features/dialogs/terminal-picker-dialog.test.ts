import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TerminalPickerDialog } from './terminal-picker-dialog';

const mounted: HTMLElement[] = [];
afterEach(() => {
  for (const element of mounted) {
    m.mount(element, null);
    element.remove();
  }
  mounted.length = 0;
});

function render(apps: { name: string; command: string }[], onChoose = vi.fn()): HTMLElement {
  const root = document.createElement('div');
  document.body.appendChild(root);
  mounted.push(root);
  m.mount(root, {
    view: () => m(TerminalPickerDialog, { open: true, apps, onChoose, onCancel: vi.fn() }),
  });
  return root;
}

describe('TerminalPickerDialog', () => {
  it('lists the configured terminals and reports the chosen one', () => {
    const onChoose = vi.fn();
    const apps = [
      { name: 'Ghostty', command: 'Ghostty' },
      { name: 'Warp', command: 'Warp' },
    ];
    const root = render(apps, onChoose);
    const buttons = [...root.querySelectorAll<HTMLButtonElement>('.fm-terminal-picker__item')];
    expect(buttons.map((button) => button.textContent)).toEqual(['Ghostty', 'Warp']);
    buttons[1]?.click();
    expect(onChoose).toHaveBeenCalledWith(apps[1]);
  });

  it('explains how to add terminals when none are configured', () => {
    const root = render([]);
    expect(root.querySelector('.fm-terminal-picker__empty')?.textContent).toContain('Settings');
  });
});
