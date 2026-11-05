import { describe, expect, it } from 'vitest';
import { escapeRegExp } from './regex';

describe('escapeRegExp', () => {
  it('escapes metacharacters so the value matches literally', () => {
    const value = 'a.b*c+d?e^f$g{h}i(j)k|l[m]n\\o';
    expect(new RegExp(escapeRegExp(value), 'u').test(value)).toBe(true);
    expect(new RegExp(escapeRegExp('a.c'), 'u').test('abc')).toBe(false);
  });

  it('leaves plain text unchanged', () => {
    expect(escapeRegExp('plain text-1')).toBe('plain text-1');
  });
});
