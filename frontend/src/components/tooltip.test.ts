import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TOOLTIP_DELAY_MS, tooltip } from './tooltip';

describe('tooltip', () => {
  afterEach(() => {
    vi.useRealTimers();
    document.querySelector('.fm-tooltip-portal')?.remove();
  });

  it('opens only after a short delay, and not at all when the pointer leaves first', () => {
    vi.useFakeTimers();
    const root = document.createElement('div');
    document.body.appendChild(root);
    m.mount(root, { view: () => tooltip('Rename', m('button', 'r')) });
    const anchor = root.querySelector<HTMLElement>('.fm-tooltip');
    const portal = () => document.querySelector<HTMLElement>('.fm-tooltip-portal');

    anchor?.dispatchEvent(new Event('mouseenter'));
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS - 1);
    expect(portal()?.style.display ?? 'none').toBe('none');
    vi.advanceTimersByTime(1);
    expect(portal()?.style.display).toBe('block');

    anchor?.dispatchEvent(new Event('mouseleave'));
    expect(portal()?.style.display).toBe('none');
    anchor?.dispatchEvent(new Event('mouseenter'));
    anchor?.dispatchEvent(new Event('mouseleave'));
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    expect(portal()?.style.display).toBe('none');

    m.mount(root, null);
    root.remove();
  });

  it('hides the popup when its control is clicked, e.g. to open a menu', () => {
    const root = document.createElement('div');
    document.body.appendChild(root);
    m.mount(root, { view: () => tooltip('More actions', m('button', 'x')) });

    const anchor = root.querySelector<HTMLElement>('.fm-tooltip');
    vi.useFakeTimers();
    anchor?.dispatchEvent(new MouseEvent('mouseover'));
    anchor?.dispatchEvent(new Event('mouseenter'));
    vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    const portal = document.querySelector<HTMLElement>('.fm-tooltip-portal');
    expect(portal?.style.display).toBe('block');

    // The focus a mouse press gives the button shows the tooltip again before the click lands.
    anchor?.dispatchEvent(new Event('focusin'));
    root.querySelector('button')?.click();
    expect(portal?.style.display).toBe('none');

    m.mount(root, null);
    root.remove();
  });
});
