import { describe, expect, it } from 'vitest';

import { tabStaysWithin, tabStops } from './tab-traversal';

/** Builds the pane content a Tab is pressed inside of. */
function pane(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

function tabOn(target: Element | null, shiftKey = false): KeyboardEvent {
  if (target === null) throw new Error('the control Tab was pressed on is missing');
  const event = new KeyboardEvent('keydown', { key: 'Tab', shiftKey, bubbles: true });
  target.dispatchEvent(event);
  return event;
}

describe('tabStops', () => {
  it('lists what the browser would tab through, in order', () => {
    const root = pane(`
      <button>One</button>
      <input><input type="hidden"><input disabled>
      <select></select><textarea></textarea>
      <a href="#x">Link</a>
      <div tabindex="0">Grid</div>
      <div tabindex="-1">Click only</div>
      <div contenteditable="true"> prose </div>
      <div hidden><button>gone</button></div>
      <div aria-hidden="true"><button>also gone</button></div>
    `);

    expect(tabStops(root).map((element) => element.tagName)).toEqual([
      'BUTTON',
      'INPUT',
      'SELECT',
      'TEXTAREA',
      'A',
      'DIV',
      'DIV',
    ]);
  });
});

describe('tabStaysWithin', () => {
  it('keeps Tab between the controls of one pane', () => {
    const root = pane('<input class="width"><input class="height"><button>Save</button>');

    expect(tabStaysWithin(tabOn(root.querySelector('.width')), root)).toBe(true);
    expect(tabStaysWithin(tabOn(root.querySelector('.height')), root)).toBe(true);
  });

  it('hands Tab to the shell at the last control, and Shift+Tab at the first', () => {
    const root = pane('<input class="width"><input class="height">');

    expect(tabStaysWithin(tabOn(root.querySelector('.height')), root)).toBe(false);
    expect(tabStaysWithin(tabOn(root.querySelector('.width'), true), root)).toBe(false);
    // …while the same keys walk back inside the pane away from the edges.
    expect(tabStaysWithin(tabOn(root.querySelector('.height'), true), root)).toBe(true);
  });

  it('leaves a Tab that did not start on a control to the shell', () => {
    const root = pane('<div class="body">nothing focused here</div><button>Save</button>');

    expect(tabStaysWithin(tabOn(root.querySelector('.body')), root)).toBe(false);
  });

  it('gives nothing to the shell when the pane has no controls to walk', () => {
    const root = pane('<div>reading matter only</div>');

    expect(tabStaysWithin(tabOn(root.firstElementChild), root)).toBe(false);
  });

  it('ignores a control outside the pane it was asked about', () => {
    const root = pane('<input class="width">');
    const outside = document.createElement('input');
    document.body.append(root, outside);

    try {
      expect(tabStaysWithin(tabOn(outside), root)).toBe(false);
    } finally {
      root.remove();
      outside.remove();
    }
  });
});
