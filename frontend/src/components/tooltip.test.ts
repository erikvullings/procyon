import m from 'mithril';
import { afterEach, describe, expect, it } from 'vitest';
import { tooltip } from './tooltip';

describe('tooltip', () => {
  afterEach(() => {
    document.querySelector('.fm-tooltip-portal')?.remove();
  });

  it('hides the popup when its control is clicked, e.g. to open a menu', () => {
    const root = document.createElement('div');
    document.body.appendChild(root);
    m.mount(root, { view: () => tooltip('More actions', m('button', 'x')) });

    const anchor = root.querySelector<HTMLElement>('.fm-tooltip');
    anchor?.dispatchEvent(new MouseEvent('mouseover'));
    anchor?.dispatchEvent(new Event('mouseenter'));
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
