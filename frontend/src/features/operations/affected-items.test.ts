import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { affectedItems } from './affected-items';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.render(root, null);
  root.remove();
});

describe('affectedItems', () => {
  it('names up to four items and summarises the rest', () => {
    const locations = ['a.txt', 'b%20c.txt', 'd', 'e', 'f', 'g'].map((name) => ({
      providerId: 'local',
      uri: `file:///tmp/${name}`,
    }));
    m.render(root, affectedItems(locations));

    const items = [...root.querySelectorAll('.fm-affected-items-list li')].map(
      (li) => li.textContent,
    );
    expect(items).toEqual(['a.txt', 'b c.txt', 'd', 'e', '+2 more']);
  });

  it('renders nothing without items', () => {
    expect(affectedItems([])).toBeUndefined();
  });
});
