import { describe, expect, it } from 'vitest';
import { mockFileDiff } from './mock-file-diff';

const bytes = (text: string) => new TextEncoder().encode(text);

describe('mockFileDiff', () => {
  it('aligns replaced, inserted and deleted lines with change starts', () => {
    const result = mockFileDiff(bytes('a\nb\nc\nd\n'), bytes('a\nB\nc\nd\ne\n'));
    expect(result.rows.map((row) => row.kind)).toEqual([
      'equal',
      'replace',
      'equal',
      'equal',
      'insert',
    ]);
    expect(result.changeStarts).toEqual([1, 4]);
    expect(result.identical).toBe(false);
  });

  it('marks the changed characters of a replaced line', () => {
    const result = mockFileDiff(bytes('value\n'), bytes('valve\n'));
    const changed = result.rows[0]?.leftSegments.filter((s) => s.changed).map((s) => s.text);
    expect(changed).toEqual(['u']);
  });

  it('ignores case and whitespace on request', () => {
    expect(mockFileDiff(bytes('Hello  x\n'), bytes('hello x\n')).identical).toBe(false);
    expect(
      mockFileDiff(bytes('Hello  x\n'), bytes('hello x\n'), {
        ignoreCase: true,
        ignoreWhitespace: true,
      }).identical,
    ).toBe(true);
  });

  it('reports binary files by first differing offset', () => {
    const result = mockFileDiff(Uint8Array.of(1, 0, 3), Uint8Array.of(1, 0, 4));
    expect(result.outcome).toBe('binary');
    expect(result.firstDifferenceOffset).toBe(2);
    expect(result.rows).toEqual([]);
    expect(mockFileDiff(Uint8Array.of(0), Uint8Array.of(0)).identical).toBe(true);
  });
});
