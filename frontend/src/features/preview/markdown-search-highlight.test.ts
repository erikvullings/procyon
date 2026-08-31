import { afterEach, describe, expect, it } from 'vitest';
import {
  findNthTextRange,
  occurrenceIndexOf,
  renderMarkdownWithHighlight,
} from './markdown-search-highlight';

describe('occurrenceIndexOf', () => {
  it('returns 0 for the first occurrence', () => {
    expect(occurrenceIndexOf('hello world', 'world', 6)).toBe(0);
  });

  it('counts prior occurrences of the same text', () => {
    expect(occurrenceIndexOf('cat sat on the cat mat', 'cat', 0)).toBe(0);
    expect(occurrenceIndexOf('cat sat on the cat mat', 'cat', 15)).toBe(1);
  });

  it('returns 0 for an empty needle', () => {
    expect(occurrenceIndexOf('hello', '', 3)).toBe(0);
  });
});

describe('findNthTextRange', () => {
  function container(html: string): HTMLElement {
    const el = document.createElement('div');
    el.innerHTML = html;
    document.body.append(el);
    return el;
  }

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('finds the first occurrence within a single text node', () => {
    const el = container('<p>hello world</p>');
    const range = findNthTextRange(el, 'world', 0);
    expect(range?.toString()).toBe('world');
  });

  it('finds the Nth occurrence across multiple text nodes, in document order', () => {
    const el = container('<p>cat sat</p><p>on the cat mat</p>');
    const range = findNthTextRange(el, 'cat', 1);
    expect(range?.toString()).toBe('cat');
    expect(range?.startContainer.parentElement?.textContent).toBe('on the cat mat');
  });

  it('returns undefined when the text cannot be found within any single text node', () => {
    // Simulates a match split across an inline formatting boundary (e.g. half inside `<strong>`).
    const el = container('<p>hello <strong>wor</strong>ld</p>');
    expect(findNthTextRange(el, 'world', 0)).toBeUndefined();
  });

  it('returns undefined when there are fewer occurrences than requested', () => {
    const el = container('<p>hello world</p>');
    expect(findNthTextRange(el, 'world', 1)).toBeUndefined();
  });
});

describe('renderMarkdownWithHighlight', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  function mount(): HTMLElement {
    const el = document.createElement('div');
    document.body.append(el);
    return el;
  }

  it('renders the HTML with no highlight when there is no active match', () => {
    const el = mount();
    renderMarkdownWithHighlight(el, '<p>hello world</p>', 'hello world', undefined, undefined);
    expect(el.innerHTML).toBe('<p>hello world</p>');
    expect(el.querySelector('mark')).toBeNull();
  });

  it('wraps a locatable match in a <mark class="fm-file-viewer-highlight">', () => {
    const el = mount();
    renderMarkdownWithHighlight(el, '<p>hello world</p>', 'hello world', 6, 5);
    const mark = el.querySelector('mark.fm-file-viewer-highlight');
    expect(mark?.textContent).toBe('world');
    expect(el.querySelector('p')?.textContent).toBe('hello world');
  });

  it('leaves the HTML unmarked when the match cannot be located (e.g. split across formatting)', () => {
    const html = '<p>hello <strong>wor</strong>ld</p>';
    const el = mount();
    renderMarkdownWithHighlight(el, html, 'hello world', 6, 5);
    expect(el.querySelector('mark')).toBeNull();
    expect(el.innerHTML).toBe(html);
  });

  it('replaces a previous mark when re-rendered for a different match', () => {
    const el = mount();
    renderMarkdownWithHighlight(
      el,
      '<p>cat sat on the cat mat</p>',
      'cat sat on the cat mat',
      0,
      3,
    );
    expect(el.querySelectorAll('mark')).toHaveLength(1);
    expect(el.querySelectorAll('mark')[0]?.textContent).toBe('cat');

    renderMarkdownWithHighlight(
      el,
      '<p>cat sat on the cat mat</p>',
      'cat sat on the cat mat',
      15,
      3,
    );
    expect(el.querySelectorAll('mark')).toHaveLength(1);
    expect(el.querySelectorAll('mark')[0]?.previousSibling?.textContent).toBe('cat sat on the ');
  });
});
