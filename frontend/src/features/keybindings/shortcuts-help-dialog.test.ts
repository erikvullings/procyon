import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ShortcutsHelpDialog } from './shortcuts-help-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('ShortcutsHelpDialog', () => {
  it('keeps its controls and footer outside the scrolling shortcut list', () => {
    m.mount(root, {
      view: () =>
        m(ShortcutsHelpDialog, {
          open: true,
          actions: [],
          keybindings: {},
          platform: 'linux',
          runtime: 'browser',
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(root.querySelector('.fm-shortcuts-help-modal')?.classList).toContain(
      'modal-fixed-footer',
    );
    const controls = root.querySelector('.fm-shortcuts-help-controls');
    const list = root.querySelector('.fm-shortcuts-help-list');
    expect(controls).not.toBeNull();
    expect(list?.querySelector('.fm-shortcuts-help-table')).not.toBeNull();
    expect(list?.contains(controls)).toBe(false);
  });

  it('focuses the filter when opened so Escape works immediately', () => {
    m.mount(root, {
      view: () =>
        m(ShortcutsHelpDialog, {
          open: true,
          actions: [],
          keybindings: {},
          platform: 'linux',
          runtime: 'browser',
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.activeElement).toBe(
      root.querySelector<HTMLInputElement>('.fm-shortcuts-help-search'),
    );
  });

  it('closes with Escape even when the shortcut capture input has focus', () => {
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(ShortcutsHelpDialog, {
          open: true,
          actions: [],
          keybindings: {},
          platform: 'linux',
          runtime: 'browser',
          onClose,
        }),
    });
    m.redraw.sync();

    const capture = root.querySelector<HTMLInputElement>('.fm-shortcuts-help-capture-input');
    if (capture === null) throw new Error('shortcut capture input missing');
    capture.focus();
    capture.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );

    expect(onClose).toHaveBeenCalledOnce();
  });
});
