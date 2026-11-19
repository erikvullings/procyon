import { describe, expect, it } from 'vitest';
import { windowEndAtBreak, windowStartAtBreak } from './text-windows';

const encode = (text: string): Uint8Array => new TextEncoder().encode(text);

describe('windowEndAtBreak', () => {
  it('ends a window after the last paragraph break', () => {
    const bytes = encode('# Title\n\nFirst paragraph.\n\nSecond paragraph that is cut');
    const end = windowEndAtBreak(bytes, 0, bytes.length);
    expect(new TextDecoder().decode(bytes.subarray(0, end))).toBe(
      '# Title\n\nFirst paragraph.\n\n',
    );
  });

  it('prefers a paragraph break over a later line break', () => {
    const bytes = encode('alpha\n\nbeta\ngamma delta');
    const end = windowEndAtBreak(bytes, 0, bytes.length);
    expect(new TextDecoder().decode(bytes.subarray(0, end))).toBe('alpha\n\n');
  });

  it('falls back to a line break, then to a word break', () => {
    const lines = encode('one line\nand a fragment');
    expect(
      new TextDecoder().decode(lines.subarray(0, windowEndAtBreak(lines, 0, lines.length))),
    ).toBe('one line\n');
    const words = encode('one two three');
    expect(
      new TextDecoder().decode(words.subarray(0, windowEndAtBreak(words, 0, words.length))),
    ).toBe('one two ');
  });

  it('keeps the whole window when no break lies above the floor', () => {
    const bytes = encode('abcdefghij');
    expect(windowEndAtBreak(bytes, 0, bytes.length)).toBe(bytes.length);
    const spaced = encode('a b cdefghijklmnop');
    // The only space is at index 1, below the floor, so the cut stays where it was.
    expect(windowEndAtBreak(spaced, 8, spaced.length)).toBe(spaced.length);
  });

  it('never ends inside a multi-byte character', () => {
    const bytes = encode('ééééé');
    // Cut two bytes into the second é (bytes 2-3), so the window must back off to byte 2.
    const end = windowEndAtBreak(bytes, 0, 3);
    expect(end).toBe(2);
    expect(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, end))).toBe('é');
  });
});

describe('windowStartAtBreak', () => {
  it('starts a window after the first paragraph break in the lead-in', () => {
    const bytes = encode('tail of a word\n\n# Heading\nbody');
    const start = windowStartAtBreak(bytes, bytes.length);
    expect(new TextDecoder().decode(bytes.subarray(start))).toBe('# Heading\nbody');
  });

  it('skips a partial character when there is no break at all', () => {
    const bytes = encode('éé'); // Start on a continuation byte, as a mid-character read would.
    const start = windowStartAtBreak(bytes.subarray(1), 2);
    expect(start).toBe(1);
    expect(new TextDecoder().decode(bytes.subarray(1 + start))).toBe('é');
  });
});
