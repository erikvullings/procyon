import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import { adjacentChange, changeBlockAt, resolveDiffPair } from './file-diff-actions';

function entry(name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  return {
    id: name,
    name,
    kind,
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
  } as EntrySummary;
}

describe('resolveDiffPair', () => {
  const a = entry('a.txt');
  const b = entry('b.txt');
  const dir = entry('d', 'directory');

  it('uses two files selected in the active pane', () => {
    expect(resolveDiffPair([a, b], [], true)).toEqual({ left: a, right: b });
  });

  it('uses one file per pane, keeping the active pane on its own side', () => {
    expect(resolveDiffPair([a], [b], true)).toEqual({ left: a, right: b });
    expect(resolveDiffPair([a], [b], false)).toEqual({ left: b, right: a });
  });

  it('refuses anything else', () => {
    expect(resolveDiffPair([a], [], true)).toBeUndefined();
    expect(resolveDiffPair([a, b, entry('c')], [], true)).toBeUndefined();
    expect(resolveDiffPair([a, dir], [], true)).toBeUndefined();
    expect(resolveDiffPair([a], [dir], true)).toBeUndefined();
    expect(resolveDiffPair([a, b], [entry('c')], true)).toEqual({ left: a, right: b });
  });
});

describe('change navigation', () => {
  const starts = [2, 10, 30];

  it('moves to the next and previous block and wraps around', () => {
    expect(adjacentChange(starts, -1, 1)).toBe(2);
    expect(adjacentChange(starts, 2, 1)).toBe(10);
    expect(adjacentChange(starts, 30, 1)).toBe(2);
    expect(adjacentChange(starts, 30, -1)).toBe(10);
    expect(adjacentChange(starts, 2, -1)).toBe(30);
    expect(adjacentChange([], 0, 1)).toBeUndefined();
  });

  it('finds the block containing a row', () => {
    expect(changeBlockAt(starts, 0)).toBe(-1);
    expect(changeBlockAt(starts, 2)).toBe(0);
    expect(changeBlockAt(starts, 29)).toBe(1);
    expect(changeBlockAt(starts, 99)).toBe(2);
  });
});
