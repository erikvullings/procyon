import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { WalkthroughPanel } from './walkthrough-panel';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('WalkthroughPanel', () => {
  it('reads the bundled guide without loading remote content', () => {
    m.mount(root, {
      view: () =>
        m(WalkthroughPanel, {
          selected: 'basics',
          onSelect: vi.fn(),
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();
    expect(root.querySelector('.fm-walkthrough-article')?.textContent).toContain('README.md');
    expect(root.querySelectorAll('.fm-walkthrough-nav button')).toHaveLength(7);
  });

  it('opens a linked guide and closes with Escape', () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    m.mount(root, {
      view: () => m(WalkthroughPanel, { selected: 'basics', onSelect, onClose }),
    });
    m.redraw.sync();
    const panel = root.querySelector<HTMLElement>('.fm-walkthrough-panel');
    expect(document.activeElement).toBe(panel);
    const link = root.querySelector<HTMLAnchorElement>('a[href="workspaces.md"]');
    const reading = root.querySelector<HTMLElement>('.fm-walkthrough-reading');
    if (reading === null) throw new Error('guide reading area missing');
    reading.scrollTo = vi.fn();
    link?.click();
    expect(onSelect).toHaveBeenCalledWith('workspaces');
    expect(reading.scrollTo).toHaveBeenCalledWith(0, 0);
    panel?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
